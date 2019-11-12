use crate::bit::BitIndex;

use super::super::alu::*;
use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::cpu::{Core, CpuExecResult};
use crate::core::arm7tdmi::psr::RegPSR;
use crate::core::arm7tdmi::{Addr, CpuError, CpuMode, CpuResult, CpuState, REG_LR, REG_PC};
use crate::core::sysbus::SysBus;

use super::*;

impl Core {
    pub fn exec_arm(&mut self, bus: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond) {
            self.S_cycle32(bus, self.pc);
            return Ok(());
        }
        match insn.fmt {
            ArmFormat::BX => self.exec_bx(bus, insn),
            ArmFormat::B_BL => self.exec_b_bl(bus, insn),
            ArmFormat::DP => self.exec_data_processing(bus, insn),
            ArmFormat::SWI => {
                self.software_interrupt(bus, insn.pc + 4, insn.swi_comment());
                Ok(())
            }
            ArmFormat::LDR_STR => self.exec_ldr_str(bus, insn),
            ArmFormat::LDR_STR_HS_IMM => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDR_STR_HS_REG => self.exec_ldr_str_hs(bus, insn),
            ArmFormat::LDM_STM => self.exec_ldm_stm(bus, insn),
            ArmFormat::MRS => self.move_from_status_register(bus, insn.rd(), insn.spsr_flag()),
            ArmFormat::MSR_REG => self.exec_msr_reg(bus, insn),
            ArmFormat::MSR_FLAGS => self.exec_msr_flags(bus, insn),
            ArmFormat::MUL_MLA => self.exec_mul_mla(bus, insn),
            ArmFormat::MULL_MLAL => self.exec_mull_mlal(bus, insn),
            ArmFormat::SWP => self.exec_arm_swp(bus, insn),
        }
    }

    /// Cycles 2S+1N
    fn exec_b_bl(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        self.S_cycle32(sb, self.pc);
        if insn.link_flag() {
            self.set_reg(REG_LR, (insn.pc + (self.word_size() as u32)) & !0b1);
        }

        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32 & !1;
        self.flush_pipeline(sb);

        Ok(())
    }

    pub fn branch_exchange(&mut self, sb: &mut SysBus, mut addr: Addr) -> CpuExecResult {
        match self.cpsr.state() {
            CpuState::ARM => self.S_cycle32(sb, self.pc),
            CpuState::THUMB => self.S_cycle16(sb, self.pc),
        }
        if addr.bit(0) {
            addr = addr & !0x1;
            self.cpsr.set_state(CpuState::THUMB);
        } else {
            addr = addr & !0x3;
            self.cpsr.set_state(CpuState::ARM);
        }

        self.pc = addr;
        self.flush_pipeline(sb); // +1S+1N

        Ok(())
    }

    /// Cycles 2S+1N
    fn exec_bx(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        self.branch_exchange(sb, self.get_reg(insn.rn()))
    }

    fn move_from_status_register(
        &mut self,
        sb: &mut SysBus,
        rd: usize,
        is_spsr: bool,
    ) -> CpuExecResult {
        let result = if is_spsr {
            self.spsr.get()
        } else {
            self.cpsr.get()
        };
        self.set_reg(rd, result);
        self.S_cycle32(sb, self.pc);
        Ok(())
    }

    fn exec_msr_reg(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        self.write_status_register(sb, insn.spsr_flag(), self.get_reg(insn.rm()))
    }

    fn write_status_register(
        &mut self,
        sb: &mut SysBus,
        is_spsr: bool,
        value: u32,
    ) -> CpuExecResult {
        let new_status_reg = RegPSR::new(value);
        match self.cpsr.mode() {
            CpuMode::User => {
                if is_spsr {
                    panic!("User mode can't access SPSR")
                }
                self.cpsr.set_flag_bits(value);
            }
            _ => {
                if is_spsr {
                    self.spsr.set(value);
                } else {
                    let t_bit = self.cpsr.state();
                    let old_mode = self.cpsr.mode();
                    self.cpsr.set(value);
                    if t_bit != self.cpsr.state() {
                        panic!("T bit changed from MSR");
                    }
                    let new_mode = new_status_reg.mode();
                    if old_mode != new_mode {
                        self.change_mode(old_mode, new_mode);
                    }
                }
            }
        }
        self.S_cycle32(sb, self.pc);
        Ok(())
    }

    fn exec_msr_flags(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        self.S_cycle32(sb, self.pc);
        let op = insn.operand2()?;
        let op = self.decode_operand2(op)?;

        let old_mode = self.cpsr.mode();
        if insn.spsr_flag() {
            self.spsr.set_flag_bits(op);
        } else {
            self.cpsr.set_flag_bits(op);
        }
        Ok(())
    }

    fn decode_operand2(&mut self, op2: BarrelShifterValue) -> CpuResult<u32> {
        match op2 {
            BarrelShifterValue::RotatedImmediate(val, amount) => {
                let result = self.ror(val, amount, self.cpsr.C(), false, true);
                Ok(result)
            }
            BarrelShifterValue::ShiftedRegister(x) => {
                let result = self.register_shift(x)?;
                Ok(result)
            }
            _ => unreachable!(),
        }
    }

    fn transfer_spsr_mode(&mut self) {
        let spsr = self.spsr;
        if self.cpsr.mode() != spsr.mode() {
            self.change_mode(self.cpsr.mode(), spsr.mode());
        }
        self.cpsr = spsr;
    }

    /// Logical/Arithmetic ALU operations
    ///
    /// Cycles: 1S+x+y (from GBATEK)
    ///         Add x=1I cycles if Op2 shifted-by-register. Add y=1S+1N cycles if Rd=R15.
    fn exec_data_processing(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        use AluOpCode::*;

        self.S_cycle32(sb, self.pc);
        let mut op1 = if insn.rn() == REG_PC {
            (insn.pc + 8)
        } else {
            self.get_reg(insn.rn())
        };

        let mut s_flag = insn.set_cond_flag();
        let opcode = insn.opcode().unwrap();

        let op2 = insn.operand2()?;
        match op2 {
            BarrelShifterValue::ShiftedRegister(_) => {
                if insn.rn() == REG_PC {
                    op1 += 4;
                }
            }
            _ => {}
        }
        let op2 = self.decode_operand2(op2)?;

        let reg_rd = insn.rd();
        if !s_flag {
            match opcode {
                TEQ => {
                    return self.write_status_register(sb, false, op2);
                }
                CMN => {
                    return self.write_status_register(sb, true, op2);
                }
                TST => return self.move_from_status_register(sb, reg_rd, false),
                CMP => return self.move_from_status_register(sb, reg_rd, true),
                _ => (),
            }
        }

        if reg_rd == REG_PC && s_flag {
            self.transfer_spsr_mode();
            s_flag = false;
        }

        let C = self.cpsr.C() as u32;
        let alu_res = if s_flag {
            let mut carry = self.bs_carry_out;
            let mut overflow = self.cpsr.V();
            let result = match opcode {
                AND | TST => op1 & op2,
                EOR | TEQ => op1 ^ op2,
                SUB | CMP => self.alu_sub_flags(op1, op2, &mut carry, &mut overflow),
                RSB => self.alu_sub_flags(op2, op1, &mut carry, &mut overflow),
                ADD | CMN => self.alu_add_flags(op1, op2, &mut carry, &mut overflow),
                ADC => self.alu_adc_flags(op1, op2, &mut carry, &mut overflow),
                SBC => self.alu_sbc_flags(op1, op2, &mut carry, &mut overflow),
                RSC => self.alu_sbc_flags(op2, op1, &mut carry, &mut overflow),
                ORR => op1 | op2,
                MOV => op2,
                BIC => op1 & (!op2),
                MVN => !op2,
            };

            self.alu_update_flags(result, opcode.is_arithmetic(), carry, overflow);

            if opcode.is_setting_flags() {
                None
            } else {
                Some(result)
            }
        } else {
            Some(match opcode {
                AND => op1 & op2,
                EOR => op1 ^ op2,
                SUB => op1.wrapping_sub(op2),
                RSB => op2.wrapping_sub(op1),
                ADD => op1.wrapping_add(op2),
                ADC => op1.wrapping_add(op2).wrapping_add(C),
                SBC => op1.wrapping_sub(op2.wrapping_add(1 - C)),
                RSC => op2.wrapping_sub(op1.wrapping_add(1 - C)),
                ORR => op1 | op2,
                MOV => op2,
                BIC => op1 & (!op2),
                MVN => !op2,
                _ => panic!("{} should be a PSR transfer", opcode),
            })
        };

        if let Some(result) = alu_res {
            if reg_rd == REG_PC {
                self.flush_pipeline(sb);
            }
            self.set_reg(reg_rd, result as u32);
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
    fn exec_ldr_str(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let mut writeback = insn.write_back_flag();
        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }
        let offset = self.get_barrel_shifted_value(insn.ldr_str_offset());
        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            writeback = true;
            addr
        };
        if writeback && insn.rd() == insn.rn() {
            writeback = false;
        }
        if insn.load_flag() {
            self.S_cycle32(sb, self.pc);
            let data = if insn.transfer_size() == 1 {
                self.N_cycle8(sb, addr);
                sb.read_8(addr) as u32
            } else {
                self.N_cycle32(sb, addr);
                self.ldr_word(addr, sb)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                self.flush_pipeline(sb);
            }
        } else {
            let value = if insn.rd() == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(insn.rd())
            };
            if insn.transfer_size() == 1 {
                self.N_cycle8(sb, addr);
                self.write_8(addr, value as u8, sb);
            } else {
                self.N_cycle32(sb, addr);
                self.write_32(addr & !0x3, value, sb);
            };
            self.N_cycle32(sb, self.pc);
        }

        if writeback {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(())
    }

    fn exec_ldr_str_hs(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let mut writeback = insn.write_back_flag();

        let mut addr = self.get_reg(insn.rn());
        if insn.rn() == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(insn.ldr_str_hs_offset().unwrap());

        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            writeback = true;
            addr
        };
        if writeback && insn.rd() == insn.rn() {
            writeback = false;
        }
        if insn.load_flag() {
            self.S_cycle32(sb, self.pc);
            let data = match insn.halfword_data_transfer_type().unwrap() {
                ArmHalfwordTransferType::SignedByte => {
                    self.N_cycle8(sb, addr);
                    sb.read_8(addr) as u8 as i8 as u32
                }
                ArmHalfwordTransferType::SignedHalfwords => {
                    self.N_cycle16(sb, addr);
                    self.ldr_sign_half(addr, sb)
                }
                ArmHalfwordTransferType::UnsignedHalfwords => {
                    self.N_cycle16(sb, addr);
                    self.ldr_half(addr, sb)
                }
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();

            if insn.rd() == REG_PC {
                self.flush_pipeline(sb);
            }
        } else {
            let value = if insn.rd() == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(insn.rd())
            };

            match insn.halfword_data_transfer_type().unwrap() {
                ArmHalfwordTransferType::UnsignedHalfwords => {
                    self.N_cycle32(sb, addr);
                    self.write_16(addr, value as u16, sb);
                    self.N_cycle32(sb, self.pc);
                }
                _ => panic!("invalid HS flags for L=0"),
            };
        }

        if writeback {
            self.set_reg(insn.rn(), effective_addr as u32)
        }

        Ok(())
    }

    fn exec_ldm_stm(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
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

        if rlist != 0 {
            if is_load {
                self.add_cycle();
                self.N_cycle32(sb, self.pc);
                for r in 0..16 {
                    let r = if ascending { r } else { 15 - r };
                    if rlist.bit(r) {
                        if r == rn {
                            writeback = false;
                        }
                        if full {
                            addr = addr.wrapping_add(step);
                        }

                        let val = sb.read_32(addr as Addr);
                        self.S_cycle32(sb, self.pc);
                        if user_bank_transfer {
                            self.set_reg_user(r, val);
                        } else {
                            self.set_reg(r, val);
                        }

                        if r == REG_PC {
                            if psr_transfer {
                                self.transfer_spsr_mode();
                            }
                            self.flush_pipeline(sb);
                        }

                        if !full {
                            addr = addr.wrapping_add(step);
                        }
                    }
                }
            } else {
                let mut first = true;
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
                        if first {
                            self.N_cycle32(sb, addr as u32);
                            first = false;
                        } else {
                            self.S_cycle32(sb, addr as u32);
                        }
                        self.write_32(addr as Addr, val, sb);

                        if !full {
                            addr = addr.wrapping_add(step);
                        }
                    }
                }
                self.N_cycle32(sb, self.pc);
            }
        } else {
            if is_load {
                let val = self.ldr_word(addr as u32, sb);
                self.set_reg(REG_PC, val & !3);
                self.flush_pipeline(sb);
            } else {
                self.write_32(addr as u32, self.pc + 4, sb);
            }
            addr = addr.wrapping_add(step * 0x10);
        }

        if writeback {
            self.set_reg(rn, addr as u32);
        }

        Ok(())
    }

    fn exec_mul_mla(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let (rd, rn, rs, rm) = (insn.rd(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        if REG_PC == rd || REG_PC == rn || REG_PC == rs || REG_PC == rm {
            return Err(CpuError::IllegalInstruction);
        }
        if rd == rm {
            return Err(CpuError::IllegalInstruction);
        }

        let op1 = self.get_reg(rm);
        let op2 = self.get_reg(rs);
        let mut result = op1.wrapping_mul(op2);

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
            self.cpsr.set_C(false);
            self.cpsr.set_V(false);
        }

        self.S_cycle32(sb, self.pc);
        Ok(())
    }

    fn exec_mull_mlal(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let (rd_hi, rd_lo, rn, rs, rm) =
            (insn.rd_hi(), insn.rd_lo(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        if REG_PC == rd_hi || REG_PC == rd_lo || REG_PC == rn || REG_PC == rs || REG_PC == rm {
            return Err(CpuError::IllegalInstruction);
        }
        if rd_hi != rd_hi && rd_hi != rm && rd_lo != rm {
            return Err(CpuError::IllegalInstruction);
        }

        let op1 = self.get_reg(rm);
        let op2 = self.get_reg(rs);
        let mut result: u64 = if insn.u_flag() {
            // signed
            (op1 as i32 as i64).wrapping_mul(op2 as i32 as i64) as u64
        } else {
            (op1 as u64).wrapping_mul(op2 as u64)
        };
        self.add_cycle();

        if insn.accumulate_flag() {
            let hi = self.get_reg(rd_hi) as u64;
            let lo = self.get_reg(rd_lo) as u64;
            result = result.wrapping_add(hi << 32 | lo);
            self.add_cycle();
        }

        self.set_reg(rd_hi, (result >> 32) as i32 as u32);
        self.set_reg(rd_lo, (result & 0xffffffff) as i32 as u32);

        let m = self.get_required_multipiler_array_cycles(self.get_reg(rs));
        for _ in 0..m {
            self.add_cycle();
        }

        if insn.set_cond_flag() {
            self.cpsr.set_N(result.bit(63));
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(false);
            self.cpsr.set_V(false);
        }

        self.S_cycle32(sb, self.pc);
        Ok(())
    }

    fn exec_arm_swp(&mut self, sb: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let base_addr = self.get_reg(insn.rn());
        self.add_cycle();
        if insn.transfer_size() == 1 {
            let t = sb.read_8(base_addr);
            self.N_cycle8(sb, base_addr);
            sb.write_8(base_addr, self.get_reg(insn.rm()) as u8);
            self.S_cycle8(sb, base_addr);
            self.set_reg(insn.rd(), t as u32);
        } else {
            let t = sb.read_32(base_addr);
            self.N_cycle32(sb, base_addr);
            sb.write_32(base_addr, self.get_reg(insn.rm()));
            self.S_cycle32(sb, base_addr);
            self.set_reg(insn.rd(), t as u32);
        }
        self.N_cycle32(sb, self.pc);
        Ok(())
    }
}
