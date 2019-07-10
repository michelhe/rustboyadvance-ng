use crate::bit::BitIndex;

use crate::arm7tdmi::alu::*;
use crate::arm7tdmi::bus::Bus;
use crate::arm7tdmi::cpu::{Core, CpuExecResult, CpuPipelineAction};
use crate::arm7tdmi::exception::Exception;
use crate::arm7tdmi::psr::RegPSR;
use crate::arm7tdmi::{Addr, CpuError, CpuResult, CpuState, DecodedInstruction, REG_PC};

use super::*;

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
            ArmFormat::LDR_STR_HS_IMM => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDR_STR_HS_REG => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDM_STM => self.exec_ldm_stm(bus, insn),
            ArmFormat::MSR_REG => self.exec_msr_reg(bus, insn),
            ArmFormat::MSR_FLAGS => self.exec_msr_flags(bus, insn),
            ArmFormat::MUL_MLA => self.exec_mul_mla(bus, insn),
            _ => Err(CpuError::UnimplementedCpuInstruction(
                insn.pc,
                insn.raw,
                DecodedInstruction::Arm(insn),
            )),
        }
    }

    /// Cycles 2S+1N
    fn exec_b_bl(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        if insn.link_flag() {
            self.set_reg(14, (insn.pc + (self.word_size() as u32)) & !0b1);
        }

        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32 & !1;

        Ok(CpuPipelineAction::Flush)
    }

    pub fn branch_exchange(&mut self, mut addr: Addr) -> CpuExecResult {
        if addr.bit(0) {
            addr = addr & !0x1;
            self.cpsr.set_state(CpuState::THUMB);
        } else {
            addr = addr & !0x3;
            self.cpsr.set_state(CpuState::ARM);
        }

        self.pc = addr;

        Ok(CpuPipelineAction::Flush)
    }

    /// Cycles 2S+1N
    fn exec_bx(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        self.branch_exchange(self.get_reg(insn.rn()))
    }

    fn exec_swi(&mut self, _bus: &mut Bus, _insn: ArmInstruction) -> CpuExecResult {
        self.exception(Exception::SoftwareInterrupt);
        Ok(CpuPipelineAction::Flush)
    }

    fn exec_msr_reg(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        self.exec_msr(insn, self.get_reg(insn.rm()))
    }

    fn exec_msr(&mut self, insn: ArmInstruction, value: u32) -> CpuExecResult {
        let new_psr = RegPSR::new(value);
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

    fn exec_msr_flags(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let op = insn.operand2()?;
        let op = self.decode_operand2(op, false)?;

        let old_mode = self.cpsr.mode();
        if insn.spsr_flag() {
            if let Some(index) = old_mode.spsr_index() {
                self.spsr[index].set_flag_bits(op);
            } else {
                panic!("tried to change spsr from invalid mode {}", old_mode)
            }
        } else {
            self.cpsr.set_flag_bits(op);
        }
        Ok(CpuPipelineAction::IncPC)
    }

    fn decode_operand2(&mut self, op2: BarrelShifterValue, set_flags: bool) -> CpuResult<u32> {
        match op2 {
            BarrelShifterValue::RotatedImmediate(imm, r) => {
                let result = imm.rotate_right(r);
                if set_flags {
                    self.cpsr.set_C((result as u32).bit(31));
                }
                Ok(result)
            }
            BarrelShifterValue::ShiftedRegister {
                reg,
                shift,
                added: _,
            } => {
                // +1I
                self.add_cycle();
                let result = self.register_shift(reg, shift)?;
                Ok(result as u32)
            }
            _ => unreachable!(),
        }
    }

    /// Logical/Arithmetic ALU operations
    ///
    /// Cycles: 1S+x+y (from GBATEK)
    ///         Add x=1I cycles if Op2 shifted-by-register. Add y=1S+1N cycles if Rd=R15.
    fn exec_data_processing(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        // TODO handle carry flag

        let mut pipeline_action = CpuPipelineAction::IncPC;

        let op1 = if insn.rn() == REG_PC {
            self.pc as i32 // prefething
        } else {
            self.get_reg(insn.rn()) as i32
        };

        let opcode = insn.opcode().unwrap();

        let set_flags = opcode.is_setting_flags() || insn.set_cond_flag();
        let op2 = insn.operand2()?;
        let op2 = self.decode_operand2(op2, set_flags)? as i32;

        if !insn.set_cond_flag() {
            match opcode {
                AluOpCode::TEQ | AluOpCode::CMN => {
                    return self.exec_msr(insn, op2 as u32);
                }
                AluOpCode::TST | AluOpCode::CMP => {
                    unimplemented!("TODO implement MRS");
                }
                _ => (),
            }
        }

        let rd = insn.rd();

        if let Some(result) = self.alu(opcode, op1, op2, set_flags) {
            self.set_reg(rd, result as u32);
            if rd == REG_PC {
                pipeline_action = CpuPipelineAction::Flush;
            }
        }

        Ok(pipeline_action)
    }

    /// Memory Load/Store
    /// Instruction                     |  Cycles       | Flags | Expl.
    /// ------------------------------------------------------------------------------
    /// LDR{cond}{B}{T} Rd,<Address>    | 1S+1N+1I+y    | ----  |  Rd=[Rn+/-<offset>]
    /// STR{cond}{B}{T} Rd,<Address>    | 2N            | ----  |  [Rn+/-<offset>]=Rd
    /// ------------------------------------------------------------------------------
    /// For LDR, add y=1S+1N if Rd=R15.
    fn exec_ldr_str(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        if insn.write_back_flag() && insn.rd() == insn.rn() {
            return Err(CpuError::IllegalInstruction);
        }

        let mut pipeline_action = CpuPipelineAction::IncPC;

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(insn.ldr_str_offset());

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
            let value = if insn.rd() == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(insn.rd())
            };
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

    fn exec_ldr_str_hs(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        if insn.write_back_flag() && insn.rd() == insn.rn() {
            return Err(CpuError::IllegalInstruction);
        }

        let mut pipeline_action = CpuPipelineAction::IncPC;

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(insn.ldr_str_hs_offset().unwrap());

        let effective_addr = (addr as i32).wrapping_add(offset) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            addr
        };

        if insn.load_flag() {
            let data = match insn.halfword_data_transfer_type().unwrap() {
                ArmHalfwordTransferType::SignedByte => self.load_8(addr, bus) as u8 as i8 as u32,
                ArmHalfwordTransferType::SignedHalfwords => {
                    self.load_16(addr, bus) as u16 as i16 as u32
                }
                ArmHalfwordTransferType::UnsignedHalfwords => self.load_16(addr, bus) as u16 as u32,
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                pipeline_action = CpuPipelineAction::Flush;
            }
        } else {
            let value = if insn.rd() == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(insn.rd())
            };

            match insn.halfword_data_transfer_type().unwrap() {
                ArmHalfwordTransferType::UnsignedHalfwords => {
                    self.store_16(addr, value as u16, bus)
                }
                _ => panic!("invalid HS flags for L=0"),
            };
        }

        if insn.write_back_flag() {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(pipeline_action)
    }

    fn exec_ldm_stm(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let full = insn.pre_index_flag();
        let ascending = insn.add_offset_flag();
        let psr_user = insn.psr_and_force_user_flag();
        let is_load = insn.load_flag();
        let mut writeback = insn.write_back_flag();
        let mut pipeline_action = CpuPipelineAction::IncPC;
        let rn = insn.rn();
        let mut addr = self.gpr[rn] as i32;

        let step: i32 = if ascending { 4 } else { -4 };
        let rlist = if ascending {
            insn.register_list()
        } else {
            let mut rlist = insn.register_list();
            rlist.reverse();
            rlist
        };

        if psr_user {
            unimplemented!("Too tired to implement the mode enforcement");
        }

        if is_load {
            if rlist.contains(&rn) {
                writeback = false;
            }
            for r in rlist {
                if full {
                    addr = addr.wrapping_add(step);
                }

                self.add_cycle();
                let val = self.load_32(addr as Addr, bus);
                self.set_reg(r, val);

                if r == REG_PC {
                    pipeline_action = CpuPipelineAction::Flush;
                }

                if !full {
                    addr = addr.wrapping_add(step);
                }
            }
        } else {
            for r in rlist {
                if full {
                    addr = addr.wrapping_add(step);
                }

                let val = if r == REG_PC {
                    insn.pc + 12
                } else {
                    self.get_reg(r)
                };
                self.store_32(addr as Addr, val, bus);

                if !full {
                    addr = addr.wrapping_add(step);
                }
            }
        }

        if writeback {
            self.set_reg(rn, addr as u32);
        }

        Ok(pipeline_action)
    }

    fn exec_mul_mla(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let rd = insn.rd();
        let rn = insn.rn();
        let rs = insn.rs();
        let rm = insn.rm();

        // check validity
        if REG_PC == rd || REG_PC == rn || REG_PC == rs || REG_PC == rm {
            return Err(CpuError::IllegalInstruction);
        }
        if rd == rm {
            return Err(CpuError::IllegalInstruction);
        }

        if !insn.accumulate_flag() {
            self.set_reg(insn.rn(), 0);
        } else {
            panic!("accumelate not implemented yet");
        }

        let op1 = self.get_reg(rm) as i32;
        let op2 = self.get_reg(rs) as i32;
        let result = (op1 * op2) as u32;
        self.set_reg(rd, result);

        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.add_cycle();
        }

        if insn.set_cond_flag() {
            self.cpsr.set_N(result.bit(31));
            self.cpsr.set_Z(result == 0);
        }

        Ok(CpuPipelineAction::IncPC)
    }
}
