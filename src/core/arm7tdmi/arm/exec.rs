use crate::bit::BitIndex;

use super::super::alu::*;
use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::cpu::{Core, CpuExecResult};
use crate::core::arm7tdmi::psr::RegPSR;
use crate::core::arm7tdmi::{
    Addr, CpuError, CpuMode, CpuResult, CpuState, DecodedInstruction, REG_PC,
};

use super::*;

impl Core {
    pub fn exec_arm(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond) {
            return Ok(());
        }
        match insn.fmt {
            ArmFormat::BX => self.exec_bx(bus, insn),
            ArmFormat::B_BL => self.exec_b_bl(bus, insn),
            ArmFormat::DP => self.exec_data_processing(bus, insn),
            ArmFormat::SWI => self.exec_swi(),
            ArmFormat::LDR_STR => self.exec_ldr_str(bus, insn),
            ArmFormat::LDR_STR_HS_IMM => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDR_STR_HS_REG => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDM_STM => self.exec_ldm_stm(bus, insn),
            ArmFormat::MRS => self.exec_mrs(insn),
            ArmFormat::MSR_REG => self.exec_msr_reg(bus, insn),
            ArmFormat::MSR_FLAGS => self.exec_msr_flags(bus, insn),
            ArmFormat::MUL_MLA => self.exec_mul_mla(bus, insn),
            ArmFormat::MULL_MLAL => self.exec_mull_mlal(bus, insn),
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
        self.flush_pipeline();

        Ok(())
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
        self.flush_pipeline();

        Ok(())
    }

    /// Cycles 2S+1N
    fn exec_bx(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        self.branch_exchange(self.get_reg(insn.rn()))
    }

    fn exec_mrs(&mut self, insn: ArmInstruction) -> CpuExecResult {
        let mode = self.cpsr.mode();
        let result = if insn.spsr_flag() {
            if let Some(index) = mode.spsr_index() {
                self.spsr[index].get()
            } else {
                panic!("tried to get spsr from invalid mode {}", mode)
            }
        } else {
            self.cpsr.get()
        };
        self.set_reg(insn.rd(), result);
        Ok(())
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
        Ok(())
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
        Ok(())
    }

    fn decode_operand2(&mut self, op2: BarrelShifterValue, set_flags: bool) -> CpuResult<u32> {
        match op2 {
            BarrelShifterValue::RotatedImmediate(val, amount) => {
                let result = self.ror(val, amount, self.cpsr.C(), false , true);
                Ok(result)
            }
            BarrelShifterValue::ShiftedRegister(x) => {
                // +1I
                self.add_cycle();
                let result = self.register_shift(x)?;
                if set_flags {
                    self.cpsr.set_C(self.bs_carry_out);
                }
                Ok(result as u32)
            }
            _ => unreachable!(),
        }
    }

    fn transfer_spsr_mode(&mut self) {
        let old_mode = self.cpsr.mode();
        if let Some(index) = old_mode.spsr_index() {
            let new_psr = self.spsr[index];
            if old_mode != new_psr.mode() {
                self.change_mode(new_psr.mode());
            }
            self.cpsr = new_psr;
        }
    }

    /// Logical/Arithmetic ALU operations
    ///
    /// Cycles: 1S+x+y (from GBATEK)
    ///         Add x=1I cycles if Op2 shifted-by-register. Add y=1S+1N cycles if Rd=R15.
    fn exec_data_processing(&mut self, _bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let op1 = if insn.rn() == REG_PC {
            self.pc as i32
        } else {
            self.get_reg(insn.rn()) as i32
        };

        let s_flag = insn.set_cond_flag();
        let opcode = insn.opcode().unwrap();

        let op2 = insn.operand2()?;
        let op2 = self.decode_operand2(op2, s_flag)? as i32;

        if !s_flag {
            match opcode {
                AluOpCode::TEQ | AluOpCode::CMN => {
                    return self.exec_msr(insn, op2 as u32);
                }
                AluOpCode::TST | AluOpCode::CMP => return self.exec_mrs(insn),
                _ => (),
            }
        }

        let rd = insn.rd();

        let alu_res = if s_flag {
            self.alu_flags(opcode, op1, op2)
        } else {
            Some(self.alu(opcode, op1, op2))
        };

        if let Some(result) = alu_res {
            if rd == REG_PC {
                self.transfer_spsr_mode();
                self.flush_pipeline();
            }
            self.set_reg(rd, result as u32);
        }

        Ok(())
    }

    /// Memory Load/Store
    /// Instruction                     |  Cycles       | Flags | Expl.
    /// ------------------------------------------------------------------------------
    /// LDR{cond}{B}{T} Rd,<Address>    | 1S+1N+1I+y    | ----  |  Rd=[Rn+/-<offset>]
    /// STR{cond}{B}{T} Rd,<Address>    | 2N            | ----  |  [Rn+/-<offset>]=Rd
    /// ------------------------------------------------------------------------------
    /// For LDR, add y=1S+1N if Rd=R15.
    fn exec_ldr_str(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let mut writeback = insn.write_back_flag();
        if writeback && insn.rd() == insn.rn() {
            writeback = false;
        }

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(insn.ldr_str_offset());

        let effective_addr = (addr as i32).wrapping_add(offset) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            writeback = true;
            addr
        };

        if insn.load_flag() {
            let data = if insn.transfer_size() == 1 {
                self.load_8(addr, bus) as u32
            } else {
                self.ldr_word(addr, bus)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                self.flush_pipeline();
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
                self.store_32(addr & !0x3, value, bus);
            };
        }

        if writeback {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(())
    }

    fn exec_ldr_str_hs(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let mut writeback = insn.write_back_flag();
        if writeback && insn.rd() == insn.rn() {
            writeback = false;
        }

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(insn.ldr_str_hs_offset().unwrap());

        let effective_addr = (addr as i32).wrapping_add(offset) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            writeback = true;
            addr
        };

        if insn.load_flag() {
            let data = match insn.halfword_data_transfer_type().unwrap() {
                ArmHalfwordTransferType::SignedByte => self.load_8(addr, bus) as u8 as i8 as u32,
                ArmHalfwordTransferType::SignedHalfwords => self.ldr_sign_half(addr, bus),
                ArmHalfwordTransferType::UnsignedHalfwords => self.ldr_half(addr, bus),
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                self.flush_pipeline();
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

        if writeback {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(())
    }

    fn exec_ldm_stm(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let full = insn.pre_index_flag();
        let ascending = insn.add_offset_flag();
        let psr_user_flag = insn.psr_and_force_user_flag();
        let is_load = insn.load_flag();
        let mut writeback = insn.write_back_flag();
        let rn = insn.rn();
        let mut addr = self.gpr[rn] as i32;

        let step: i32 = if ascending { 4 } else { -4 };
        let rlist = insn.register_list();

        if psr_user_flag {
            match self.cpsr.mode() {
                CpuMode::User | CpuMode::System => {
                    panic!("LDM/STM with S bit in unprivileged mode")
                }
                _ => {}
            };
        }

        let user_bank_transfer = if psr_user_flag {
            if is_load {
                !rlist.bit(REG_PC)
            } else {
                true
            }
        } else {
            false
        };

        let psr_transfer = psr_user_flag & is_load & rlist.bit(REG_PC);

        if is_load {
            for r in 0..16 {
                let r = if ascending { r } else { 15 - r };
                if rlist.bit(r) {
                    if r == rn {
                        writeback = false;
                    }
                    if full {
                        addr = addr.wrapping_add(step);
                    }

                    self.add_cycle();
                    let val = self.load_32(addr as Addr, bus);
                    if user_bank_transfer {
                        self.set_reg_user(r, val);
                    } else {
                        self.set_reg(r, val);
                    }

                    if r == REG_PC {
                        if psr_transfer {
                            self.transfer_spsr_mode();
                        }
                        self.flush_pipeline();
                    }

                    if !full {
                        addr = addr.wrapping_add(step);
                    }
                }
            }
        } else {
            for r in 0..16 {
                let r = if ascending { r } else { 15 - r };
                if rlist.bit(r) {
                    if full {
                        addr = addr.wrapping_add(step);
                    }

                    let val = if r == REG_PC {
                        insn.pc + 12
                    } else {
                        if user_bank_transfer {
                            self.get_reg_user(r)
                        } else {
                            self.get_reg(r)
                        }
                    };
                    self.store_32(addr as Addr, val, bus);

                    if !full {
                        addr = addr.wrapping_add(step);
                    }
                }
            }
        }

        if writeback {
            self.set_reg(rn, addr as u32);
        }

        Ok(())
    }

    fn exec_mul_mla(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let (rd, rn, rs, rm) = (insn.rd(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        if REG_PC == rd || REG_PC == rn || REG_PC == rs || REG_PC == rm {
            return Err(CpuError::IllegalInstruction);
        }
        if rd == rm {
            return Err(CpuError::IllegalInstruction);
        }

        let op1 = self.get_reg(rm) as i32;
        let op2 = self.get_reg(rs) as i32;
        let mut result = (op1 * op2) as u32;

        if insn.accumulate_flag() {
            result = result.wrapping_add(self.get_reg(rn));
            self.add_cycle();
        }

        self.set_reg(rd, result);

        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.add_cycle();
        }

        if insn.set_cond_flag() {
            self.cpsr.set_N((result as i32) < 0);
            self.cpsr.set_Z(result == 0);
        }

        Ok(())
    }

    fn exec_mull_mlal(&mut self, bus: &mut Bus, insn: ArmInstruction) -> CpuExecResult {
        let (rd_hi, rd_lo, rn, rs, rm) =
            (insn.rd_hi(), insn.rd_lo(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        if REG_PC == rd_hi || REG_PC == rd_lo || REG_PC == rn || REG_PC == rs || REG_PC == rm {
            return Err(CpuError::IllegalInstruction);
        }
        if rd_hi != rd_hi && rd_hi != rm && rd_lo != rm {
            return Err(CpuError::IllegalInstruction);
        }

        let op1 = self.get_reg(rm) as u64;
        let op2 = self.get_reg(rs) as u64;
        let mut result: u64 = if insn.u_flag() {
            // signed
            (op1 as i64).wrapping_mul(op2 as i64) as u64
        } else {
            op1.wrapping_mul(op2)
        };
        self.add_cycle();

        if insn.accumulate_flag() {
            result = result.wrapping_add(self.get_reg(rn) as u64);
            self.add_cycle();
        }

        self.set_reg(rd_hi, (result >> 32) as u32);
        self.set_reg(rd_lo, (result & 0xffffffff) as u32);

        let m = self.get_required_multipiler_array_cycles(self.get_reg(rs) as i32);
        for _ in 0..m {
            self.add_cycle();
        }

        if insn.set_cond_flag() {
            self.cpsr.set_N((result as i64) < 0);
            self.cpsr.set_Z(result == 0);
        }

        Ok(())
    }
}
