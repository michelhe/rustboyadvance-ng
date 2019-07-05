use crate::bit::BitIndex;

use crate::arm7tdmi::bus::Bus;
use crate::arm7tdmi::cpu::{Core, CpuExecResult, CpuPipelineAction};
use crate::arm7tdmi::exception::Exception;
use crate::arm7tdmi::psr::RegPSR;
use crate::arm7tdmi::{Addr, CpuError, CpuResult, CpuState, DecodedInstruction, REG_PC};

use super::{
    ArmFormat, ArmInstruction, ArmOpCode, ArmRegisterShift, ArmShiftType, ArmShiftedValue,
};

impl Core {
    pub fn exec_arm(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond) {
            return Ok(CpuPipelineAction::IncPC);
        }
        match insn.fmt {
            ArmFormat::BX => self.exec_bx(bus, insn),
            ArmFormat::B_BL => self.exec_b_bl(bus, insn),
            ArmFormat::DP => self.exec_data_processing(bus, insn),
            ArmFormat::SWI => self.exec_swi(bus, insn),
            ArmFormat::LDR_STR => self.exec_ldr_str(bus, insn),
            ArmFormat::MSR_REG => self.exec_msr_reg(bus, insn),
            _ => Err(CpuError::UnimplementedCpuInstruction(
                insn.pc,
                insn.raw,
                DecodedInstruction::Arm(insn),
            )),
        }
    }

    /// Cycles 2S+1N
    fn exec_b_bl(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuResult<CpuPipelineAction> {
        if insn.link_flag() {
            self.set_reg(14, (insn.pc + (self.word_size() as u32)) & !0b1);
        }

        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32 & !1;

        Ok(CpuPipelineAction::Flush)
    }

    /// Cycles 2S+1N
    fn exec_bx(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuResult<CpuPipelineAction> {
        let rn = self.get_reg(insn.rn());
        if rn.bit(0) {
            self.cpsr.set_state(CpuState::THUMB);
        } else {
            self.cpsr.set_state(CpuState::ARM);
        }

        self.pc = rn & !1;

        Ok(CpuPipelineAction::Flush)
    }

    fn exec_swi(&mut self, _bus: &mut Bus, _insn: ArmInstruction) -> CpuResult<CpuPipelineAction> {
        self.exception(Exception::SoftwareInterrupt);
        Ok(CpuPipelineAction::Flush)
    }

    fn exec_msr_reg(
        &mut self,
        _bus: &mut Bus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        let new_psr = RegPSR::new(self.get_reg(insn.rm()));
        let old_mode = self.cpsr.mode();
        if insn.spsr_flag() {
            if let Some(index) = old_mode.spsr_index() {
                self.spsr[index] = new_psr;
            } else {
                panic!("tried to change spsr from invalid mode {}", old_mode)
            }
        } else {
            if old_mode != new_psr.mode() {
                self.change_mode(new_psr.mode());
            }
            self.cpsr = new_psr;
        }
        Ok(CpuPipelineAction::IncPC)
    }

    fn barrel_shift(&mut self, val: i32, amount: u32, shift: ArmShiftType) -> i32 {
        match shift {
            ArmShiftType::LSL => {
                if amount < 32 {
                    self.cpsr.set_C(val.wrapping_shr(32 - amount) & 1 == 1);
                } else {
                    if amount == 32 {
                        self.cpsr.set_C(val & 1 == 1);
                    } else {
                        self.cpsr.set_C(false)
                    }
                }
                val.wrapping_shl(amount)
            }
            ArmShiftType::LSR => {
                if amount < 32 {
                    self.cpsr.set_C(val.wrapping_shr(amount - 1) & 1 == 1);
                } else {
                    self.cpsr.set_C(false);
                }
                (val as u32).wrapping_shr(amount) as i32
            }
            ArmShiftType::ASR => {
                if amount < 32 {
                    self.cpsr.set_C(val.wrapping_shr(amount - 1) & 1 == 1);
                } else {
                    self.cpsr.set_C(val >> 31 == 1);
                }
                val.wrapping_shr(amount)
            }
            ArmShiftType::ROR => {
                let amount = amount % 32;
                let result = val.rotate_right(amount);
                self.cpsr.set_C((result >> 1)  &1 == 1);
                result
            }
        }
    }

    pub fn register_shift(&mut self, reg: usize, shift: ArmRegisterShift) -> CpuResult<i32> {
        let val = self.get_reg(reg) as i32;
        match shift {
            ArmRegisterShift::ShiftAmount(amount, shift) => {
                Ok(self.barrel_shift(val, amount, shift))
            }
            ArmRegisterShift::ShiftRegister(reg, shift) => {
                if reg != REG_PC {
                    Ok(self.barrel_shift(val, self.get_reg(reg) & 0xff, shift))
                } else {
                    Err(CpuError::IllegalInstruction)
                }
            }
        }
    }

    fn alu_sub_update_carry(a: i32, b: i32, carry: &mut bool) -> i32 {
        let res = a.wrapping_sub(b);
        *carry = res > a;
        res
    }

    fn alu_add_update_carry(a: i32, b: i32, carry: &mut bool) -> i32 {
        let res = a.wrapping_add(b);
        *carry = res < a;
        res
    }

    #[allow(non_snake_case)]
    pub fn alu(
        &mut self,
        opcode: ArmOpCode,
        op1: i32,
        op2: i32,
        set_cond_flags: bool,
    ) -> Option<i32> {
        let C = self.cpsr.C() as i32;

        let mut carry = self.cpsr.C();
        let overflow = self.cpsr.V();

        let result = match opcode {
            ArmOpCode::AND | ArmOpCode::TST => op1 & op2,
            ArmOpCode::EOR | ArmOpCode::TEQ => op1 ^ op2,
            ArmOpCode::SUB | ArmOpCode::CMP => Self::alu_sub_update_carry(op1, op2, &mut carry),
            ArmOpCode::RSB => Self::alu_sub_update_carry(op2, op1, &mut carry),
            ArmOpCode::ADD | ArmOpCode::CMN => Self::alu_add_update_carry(op1, op2, &mut carry),
            ArmOpCode::ADC => Self::alu_add_update_carry(op1, op2.wrapping_add(C), &mut carry),
            ArmOpCode::SBC => Self::alu_add_update_carry(op1, op2.wrapping_sub(1 - C), &mut carry),
            ArmOpCode::RSC => Self::alu_add_update_carry(op2, op1.wrapping_sub(1 - C), &mut carry),
            ArmOpCode::ORR => op1 | op2,
            ArmOpCode::MOV => op2,
            ArmOpCode::BIC => op1 & (!op2),
            ArmOpCode::MVN => !op2,
        };

        if set_cond_flags {
            self.cpsr.set_N(result < 0);
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(carry);
            self.cpsr.set_V(overflow);
        }

        match opcode {
            ArmOpCode::TST | ArmOpCode::TEQ | ArmOpCode::CMP | ArmOpCode::CMN => None,
            _ => Some(result),
        }
    }

    /// Logical/Arithmetic ALU operations
    ///
    /// Cycles: 1S+x+y (from GBATEK)
    ///         Add x=1I cycles if Op2 shifted-by-register. Add y=1S+1N cycles if Rd=R15.
    fn exec_data_processing(
        &mut self,
        _bus: &mut Bus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        // TODO handle carry flag

        let mut pipeline_action = CpuPipelineAction::IncPC;

        let op1 = if insn.rn() == REG_PC {
            self.pc as i32 // prefething
        } else {
            self.get_reg(insn.rn()) as i32
        };
        let op2 = insn.operand2()?;

        let rd = insn.rd();

        let op2: i32 = match op2 {
            ArmShiftedValue::RotatedImmediate(immediate, rotate) => {
                Ok(immediate.rotate_right(rotate) as i32)
            }
            ArmShiftedValue::ShiftedRegister {
                reg,
                shift,
                added: _,
            } => {
                // +1I
                self.add_cycle();
                self.register_shift(reg, shift)
            }
            _ => unreachable!(),
        }?;

        let opcode = insn.opcode().unwrap();
        let set_flags = opcode.is_setting_flags() || insn.set_cond_flag();
        if let Some(result) = self.alu(opcode, op1, op2, set_flags) {
            self.set_reg(rd, result as u32);
            if rd == REG_PC {
                pipeline_action = CpuPipelineAction::Flush;
            }
        }

        Ok(pipeline_action)
    }

    fn get_rn_offset(&mut self, insn: &ArmInstruction) -> i32 {
        // TODO decide if error handling or panic here
        match insn.ldr_str_offset().unwrap() {
            ArmShiftedValue::ImmediateValue(offset) => offset,
            ArmShiftedValue::ShiftedRegister {
                reg,
                shift,
                added: Some(added),
            } => {
                let abs = self.register_shift(reg, shift).unwrap();
                if added {
                    abs
                } else {
                    -abs
                }
            }
            _ => panic!("bad barrel shift"),
        }
    }

    /// Memory Load/Store
    /// Instruction                     |  Cycles       | Flags | Expl.
    /// ------------------------------------------------------------------------------
    /// LDR{cond}{B}{T} Rd,<Address>    | 1S+1N+1I+y    | ----  |  Rd=[Rn+/-<offset>]
    /// STR{cond}{B}{T} Rd,<Address>    | 2N            | ----  |  [Rn+/-<offset>]=Rd
    /// ------------------------------------------------------------------------------
    /// For LDR, add y=1S+1N if Rd=R15.
    fn exec_ldr_str(
        &mut self,
        bus: &mut Bus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        if insn.write_back_flag() && insn.rd() == insn.rn() {
            return Err(CpuError::IllegalInstruction);
        }

        let mut pipeline_action = CpuPipelineAction::IncPC;

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_rn_offset(&insn);

        let effective_addr = (addr as i32).wrapping_add(offset) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            addr
        };

        if insn.load_flag() {
            let data = if insn.transfer_size() == 1 {
                self.load_8(addr, bus) as u32
            } else {
                self.load_32(addr, bus)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                pipeline_action = CpuPipelineAction::Flush;
            }
        } else {
            let value = self.get_reg(insn.rd());
            if insn.transfer_size() == 1 {
                self.store_8(addr, value as u8, bus);
            } else {
                self.store_32(addr, value, bus);
            };
        }

        if insn.write_back_flag() {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(pipeline_action)
    }
}
