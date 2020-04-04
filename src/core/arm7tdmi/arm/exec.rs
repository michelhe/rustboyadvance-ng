use crate::bit::BitIndex;

use super::super::alu::*;
use crate::core::arm7tdmi::psr::RegPSR;
use crate::core::arm7tdmi::CpuAction;
use crate::core::arm7tdmi::{Addr, Core, CpuMode, CpuState, REG_LR, REG_PC};
use crate::core::sysbus::SysBus;
use crate::core::Bus;

use super::*;

impl Core {
    pub fn exec_arm(&mut self, bus: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        match insn.fmt {
            ArmFormat::BX => self.exec_arm_bx(bus, insn),
            ArmFormat::B_BL => self.exec_arm_b_bl(bus, insn),
            ArmFormat::DP => self.exec_arm_data_processing(bus, insn),
            ArmFormat::SWI => self.exec_arm_swi(bus, insn),
            ArmFormat::LDR_STR => self.exec_arm_ldr_str(bus, insn),
            ArmFormat::LDR_STR_HS_IMM => self.exec_arm_ldr_str_hs(bus, insn),
            ArmFormat::LDR_STR_HS_REG => self.exec_arm_ldr_str_hs(bus, insn),
            ArmFormat::LDM_STM => self.exec_arm_ldm_stm(bus, insn),
            ArmFormat::MRS => self.exec_arm_mrs(bus, insn),
            ArmFormat::MSR_REG => self.exec_arm_msr_reg(bus, insn),
            ArmFormat::MSR_FLAGS => self.exec_arm_msr_flags(bus, insn),
            ArmFormat::MUL_MLA => self.exec_arm_mul_mla(bus, insn),
            ArmFormat::MULL_MLAL => self.exec_arm_mull_mlal(bus, insn),
            ArmFormat::SWP => self.exec_arm_swp(bus, insn),
            ArmFormat::Undefined => panic!("Undefined instruction "),
        }
    }

    pub fn arm_undefined(&mut self, _: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        panic!(
            "executing undefind arm instruction {:08x} at @{:08x}",
            insn.raw, insn.pc
        )
    }

    /// Cycles 2S+1N
    pub fn exec_arm_b_bl(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.S_cycle32(sb, self.pc);
        if insn.link_flag() {
            self.set_reg(REG_LR, (insn.pc + (self.word_size() as u32)) & !0b1);
        }

        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32 & !1;

        self.reload_pipeline32(sb);
        CpuAction::FlushPipeline
    }

    pub fn branch_exchange(&mut self, sb: &mut SysBus, mut addr: Addr) -> CpuAction {
        match self.cpsr.state() {
            CpuState::ARM => self.S_cycle32(sb, self.pc),
            CpuState::THUMB => self.S_cycle16(sb, self.pc),
        }
        if addr.bit(0) {
            addr = addr & !0x1;
            self.cpsr.set_state(CpuState::THUMB);
            self.pc = addr;
            self.reload_pipeline16(sb);
        } else {
            addr = addr & !0x3;
            self.cpsr.set_state(CpuState::ARM);
            self.pc = addr;
            self.reload_pipeline32(sb);
        }

        CpuAction::FlushPipeline
    }

    /// Cycles 2S+1N
    pub fn exec_arm_bx(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.branch_exchange(sb, self.get_reg(insn.rn()))
    }

    fn move_from_status_register(
        &mut self,
        sb: &mut SysBus,
        rd: usize,
        is_spsr: bool,
    ) -> CpuAction {
        let result = if is_spsr {
            self.spsr.get()
        } else {
            self.cpsr.get()
        };
        self.set_reg(rd, result);
        self.S_cycle32(sb, self.pc);

        CpuAction::AdvancePC
    }

    pub fn exec_arm_mrs(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.move_from_status_register(sb, insn.rd(), insn.spsr_flag())
    }

    pub fn exec_arm_msr_reg(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.write_status_register(sb, insn.spsr_flag(), self.get_reg(insn.rm()))
    }

    fn write_status_register(&mut self, sb: &mut SysBus, is_spsr: bool, value: u32) -> CpuAction {
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

        CpuAction::AdvancePC
    }

    pub fn exec_arm_msr_flags(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.S_cycle32(sb, self.pc);
        let op = insn.operand2();
        let op = self.decode_operand2(&op);

        if insn.spsr_flag() {
            self.spsr.set_flag_bits(op);
        } else {
            self.cpsr.set_flag_bits(op);
        }
        CpuAction::AdvancePC
    }

    fn decode_operand2(&mut self, op2: &BarrelShifterValue) -> u32 {
        match op2 {
            BarrelShifterValue::RotatedImmediate(val, amount) => {
                self.ror(*val, *amount, self.cpsr.C(), false, true)
            }
            BarrelShifterValue::ShiftedRegister(x) => self.register_shift(&x),
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
    pub fn exec_arm_data_processing(
        &mut self,
        sb: &mut SysBus,
        insn: &ArmInstruction,
    ) -> CpuAction {
        use AluOpCode::*;

        self.S_cycle32(sb, self.pc);
        let mut op1 = if insn.rn() == REG_PC {
            insn.pc + 8
        } else {
            self.get_reg(insn.rn())
        };

        let mut s_flag = insn.set_cond_flag();
        let opcode = insn.opcode().unwrap();

        let op2 = insn.operand2();
        match op2 {
            BarrelShifterValue::ShiftedRegister(shifted_reg) => {
                if insn.rn() == REG_PC && shifted_reg.is_shifted_by_reg() {
                    op1 += 4;
                }
            }
            _ => {}
        }
        let op2 = self.decode_operand2(&op2);

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

        let carry = self.cpsr.C() as u32;
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
                ADC => op1.wrapping_add(op2).wrapping_add(carry),
                SBC => op1.wrapping_sub(op2.wrapping_add(1 - carry)),
                RSC => op2.wrapping_sub(op1.wrapping_add(1 - carry)),
                ORR => op1 | op2,
                MOV => op2,
                BIC => op1 & (!op2),
                MVN => !op2,
                _ => panic!("{} should be a PSR transfer", opcode),
            })
        };

        let mut result = CpuAction::AdvancePC;
        if let Some(alu_res) = alu_res {
            self.set_reg(reg_rd, alu_res as u32);
            if reg_rd == REG_PC {
                // T bit might have changed
                match self.cpsr.state() {
                    CpuState::ARM => self.reload_pipeline32(sb),
                    CpuState::THUMB => self.reload_pipeline16(sb),
                };
                result = CpuAction::FlushPipeline;
            }
        }

        result
    }

    /// Memory Load/Store
    /// Instruction                     |  Cycles       | Flags | Expl.
    /// ------------------------------------------------------------------------------
    /// LDR{cond}{B}{T} Rd,<Address>    | 1S+1N+1I+y    | ----  |  Rd=[Rn+/-<offset>]
    /// STR{cond}{B}{T} Rd,<Address>    | 2N            | ----  |  [Rn+/-<offset>]=Rd
    /// ------------------------------------------------------------------------------
    /// For LDR, add y=1S+1N if Rd=R15.
    pub fn exec_arm_ldr_str(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let mut result = CpuAction::AdvancePC;

        let load = insn.load_flag();
        let pre_index = insn.pre_index_flag();
        let writeback = insn.write_back_flag();
        let base_reg = insn.rn();
        let dest_reg = insn.rd();
        let mut addr = self.get_reg(base_reg);
        if base_reg == REG_PC {
            addr = insn.pc + 8; // prefetching
        }
        let offset = self.get_barrel_shifted_value(&insn.ldr_str_offset());
        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;

        // TODO - confirm this
        let old_mode = self.cpsr.mode();
        if !pre_index && writeback {
            self.change_mode(old_mode, CpuMode::User);
        }

        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            addr
        };

        if load {
            self.S_cycle32(sb, self.pc);
            let data = if insn.transfer_size() == 1 {
                self.N_cycle8(sb, addr);
                sb.read_8(addr) as u32
            } else {
                self.N_cycle32(sb, addr);
                self.ldr_word(addr, sb)
            };

            self.set_reg(dest_reg, data);

            // +1I
            self.add_cycle();

            if dest_reg == REG_PC {
                self.reload_pipeline32(sb);
                result = CpuAction::FlushPipeline;
            }
        } else {
            let value = if dest_reg == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(dest_reg)
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

        if !load || base_reg != dest_reg {
            if !pre_index {
                self.set_reg(base_reg, effective_addr);
            } else if insn.write_back_flag() {
                self.set_reg(base_reg, effective_addr);
            }
        }

        if !pre_index && insn.write_back_flag() {
            self.change_mode(self.cpsr.mode(), old_mode);
        }

        result
    }

    pub fn exec_arm_ldr_str_hs(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let mut result = CpuAction::AdvancePC;

        let load = insn.load_flag();
        let pre_index = insn.pre_index_flag();
        let writeback = insn.write_back_flag();
        let base_reg = insn.rn();
        let dest_reg = insn.rd();
        let mut addr = self.get_reg(base_reg);
        if base_reg == REG_PC {
            addr = insn.pc + 8; // prefetching
        }

        let offset = self.get_barrel_shifted_value(&insn.ldr_str_hs_offset().unwrap());

        // TODO - confirm this
        let old_mode = self.cpsr.mode();
        if !pre_index && writeback {
            self.change_mode(old_mode, CpuMode::User);
        }

        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;
        addr = if insn.pre_index_flag() {
            effective_addr
        } else {
            addr
        };

        if load {
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

            self.set_reg(dest_reg, data);

            // +1I
            self.add_cycle();

            if dest_reg == REG_PC {
                self.reload_pipeline32(sb);
                result = CpuAction::FlushPipeline;
            }
        } else {
            let value = if dest_reg == REG_PC {
                insn.pc + 12
            } else {
                self.get_reg(dest_reg)
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

        if !load || base_reg != dest_reg {
            if !pre_index {
                self.set_reg(base_reg, effective_addr);
            } else if insn.write_back_flag() {
                self.set_reg(base_reg, effective_addr);
            }
        }

        result
    }

    pub fn exec_arm_ldm_stm(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let mut result = CpuAction::AdvancePC;

        let mut full = insn.pre_index_flag();
        let ascending = insn.add_offset_flag();
        let s_flag = insn.raw.bit(22);
        let is_load = insn.load_flag();
        let mut writeback = insn.write_back_flag();
        let base_reg = insn.rn();
        let mut base_addr = self.get_reg(base_reg);

        let rlist = insn.register_list();

        if s_flag {
            match self.cpsr.mode() {
                CpuMode::User | CpuMode::System => {
                    panic!("LDM/STM with S bit in unprivileged mode")
                }
                _ => {}
            };
        }

        let user_bank_transfer = if s_flag {
            if is_load {
                !rlist.bit(REG_PC)
            } else {
                true
            }
        } else {
            false
        };

        let old_mode = self.cpsr.mode();
        if user_bank_transfer {
            self.change_mode(old_mode, CpuMode::User);
        }

        let psr_transfer = s_flag & is_load & rlist.bit(REG_PC);

        let rlist_count = rlist.count_ones();

        let old_base = base_addr;

        if rlist != 0 && !ascending {
            base_addr = base_addr.wrapping_sub(rlist_count * 4);
            if writeback {
                self.set_reg(base_reg, base_addr);
                writeback = false;
            }
            full = !full;
        }

        let mut addr = base_addr;

        if rlist != 0 {
            if is_load {
                self.add_cycle();
                self.N_cycle32(sb, self.pc);
                for r in 0..16 {
                    if rlist.bit(r) {
                        if r == base_reg {
                            writeback = false;
                        }
                        if full {
                            addr = addr.wrapping_add(4);
                        }

                        let val = sb.read_32(addr);
                        self.S_cycle32(sb, self.pc);

                        self.set_reg(r, val);

                        if r == REG_PC {
                            if psr_transfer {
                                self.transfer_spsr_mode();
                            }
                            self.reload_pipeline32(sb);
                            result = CpuAction::FlushPipeline;
                        }

                        if !full {
                            addr = addr.wrapping_add(4);
                        }
                    }
                }
            } else {
                let mut first = true;
                for r in 0..16 {
                    if rlist.bit(r) {
                        let val = if r != base_reg {
                            if r == REG_PC {
                                insn.pc + 12
                            } else {
                                self.get_reg(r)
                            }
                        } else {
                            if first {
                                old_base
                            } else {
                                let x = rlist_count * 4;
                                if ascending {
                                    old_base + x
                                } else {
                                    old_base - x
                                }
                            }
                        };

                        if full {
                            addr = addr.wrapping_add(4);
                        }

                        if first {
                            self.N_cycle32(sb, addr);
                            first = false;
                        } else {
                            self.S_cycle32(sb, addr);
                        }
                        self.write_32(addr, val, sb);

                        if !full {
                            addr = addr.wrapping_add(4);
                        }
                    }
                }
                self.N_cycle32(sb, self.pc);
            }
        } else {
            if is_load {
                let val = self.ldr_word(addr, sb);
                self.set_reg(REG_PC, val & !3);
                self.reload_pipeline32(sb);
                result = CpuAction::FlushPipeline;
            } else {
                self.write_32(addr, self.pc + 4, sb);
            }
            addr = addr.wrapping_add(0x40);
        }

        if user_bank_transfer {
            self.change_mode(self.cpsr.mode(), old_mode);
        }

        if writeback {
            self.set_reg(base_reg, addr as u32);
        }

        result
    }

    pub fn exec_arm_mul_mla(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let (rd, rn, rs, rm) = (insn.rd(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        assert!(!(REG_PC == rd || REG_PC == rn || REG_PC == rs || REG_PC == rm));
        assert!(rd != rm);

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

        CpuAction::AdvancePC
    }

    pub fn exec_arm_mull_mlal(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let (rd_hi, rd_lo, rn, rs, rm) =
            (insn.rd_hi(), insn.rd_lo(), insn.rn(), insn.rs(), insn.rm());

        // check validity
        assert!(
            !(REG_PC == rd_hi || REG_PC == rd_lo || REG_PC == rn || REG_PC == rs || REG_PC == rm)
        );
        assert!(!(rd_hi != rd_hi && rd_hi != rm && rd_lo != rm));

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

        CpuAction::AdvancePC
    }

    pub fn exec_arm_swp(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        let base_addr = self.get_reg(insn.rn());
        if insn.transfer_size() == 1 {
            let t = sb.read_8(base_addr);
            self.N_cycle8(sb, base_addr);
            sb.write_8(base_addr, self.get_reg(insn.rm()) as u8);
            self.S_cycle8(sb, base_addr);
            self.set_reg(insn.rd(), t as u32);
        } else {
            let t = self.ldr_word(base_addr, sb);
            self.N_cycle32(sb, base_addr);
            self.write_32(base_addr, self.get_reg(insn.rm()), sb);
            self.S_cycle32(sb, base_addr);
            self.set_reg(insn.rd(), t as u32);
        }
        self.add_cycle();
        self.N_cycle32(sb, self.pc);

        CpuAction::AdvancePC
    }

    pub fn exec_arm_swi(&mut self, sb: &mut SysBus, insn: &ArmInstruction) -> CpuAction {
        self.software_interrupt(sb, self.pc - 4, insn.swi_comment());
        CpuAction::FlushPipeline
    }
}
