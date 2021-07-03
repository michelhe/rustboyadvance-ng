use crate::bit::BitIndex;

use super::super::alu::*;
use crate::arm7tdmi::psr::RegPSR;
use crate::arm7tdmi::CpuAction;
use crate::arm7tdmi::{Addr, Core, CpuMode, CpuState, REG_LR, REG_PC};

use super::super::memory::{MemoryAccess, MemoryInterface};
use MemoryAccess::*;

use super::ArmDecodeHelper;
use super::*;

impl<I: MemoryInterface> Core<I> {
    pub fn arm_undefined(&mut self, insn: u32) -> CpuAction {
        panic!(
            "executing undefined arm instruction {:08x} at @{:08x}",
            insn,
            self.pc_arm()
        )
    }

    /// Branch and Branch with Link (B, BL)
    /// Execution Time: 2S + 1N
    pub fn exec_arm_b_bl<const LINK: bool>(&mut self, insn: u32) -> CpuAction {
        if LINK {
            self.set_reg(REG_LR, (self.pc_arm() + (self.word_size() as u32)) & !0b1);
        }
        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32 & !1;

        self.reload_pipeline32(); // Implies 2S + 1N
        CpuAction::PipelineFlushed
    }

    pub fn branch_exchange(&mut self, mut addr: Addr) -> CpuAction {
        if addr.bit(0) {
            addr = addr & !0x1;
            self.cpsr.set_state(CpuState::THUMB);
            self.pc = addr;
            self.reload_pipeline16();
        } else {
            addr = addr & !0x3;
            self.cpsr.set_state(CpuState::ARM);
            self.pc = addr;
            self.reload_pipeline32();
        }
        CpuAction::PipelineFlushed
    }
    /// Branch and Exchange (BX)
    /// Cycles 2S+1N
    pub fn exec_arm_bx(&mut self, insn: u32) -> CpuAction {
        self.branch_exchange(self.get_reg(insn.bit_range(0..4) as usize))
    }

    /// Move from status register
    /// 1S
    pub fn exec_arm_mrs<const SPSR_FLAG: bool>(&mut self, insn: u32) -> CpuAction {
        let rd = insn.bit_range(12..16) as usize;
        let result = if SPSR_FLAG {
            self.spsr.get()
        } else {
            self.cpsr.get()
        };
        self.set_reg(rd, result);

        CpuAction::AdvancePC(Seq)
    }

    /// Move to status register
    /// 1S
    pub fn exec_arm_transfer_to_status<const IMM: bool, const SPSR_FLAG: bool>(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let value = if IMM {
            let immediate = insn & 0xff;
            let rotate = 2 * insn.bit_range(8..12);
            let mut carry = self.cpsr.C();
            let v = self.ror(immediate, rotate, &mut carry, false, true);
            self.cpsr.set_C(carry);
            v
        } else {
            self.get_reg((insn & 0b1111) as usize)
        };

        let f = insn.bit(19);
        let s = insn.bit(18);
        let x = insn.bit(17);
        let c = insn.bit(16);

        let mut mask = 0;
        if f {
            mask |= 0xff << 24;
        }
        if s {
            mask |= 0xff << 16;
        }
        if x {
            mask |= 0xff << 8;
        }
        if c {
            mask |= 0xff << 0;
        }

        match self.cpsr.mode() {
            CpuMode::User => {
                if SPSR_FLAG {
                    panic!("User mode can't access SPSR")
                }
                self.cpsr.set_flag_bits(value);
            }
            _ => {
                if SPSR_FLAG {
                    self.spsr.set(value);
                } else {
                    let old_mode = self.cpsr.mode();
                    let new_psr = RegPSR::new((self.cpsr.get() & !mask) | (value & mask));
                    let new_mode = new_psr.mode();
                    if old_mode != new_mode {
                        self.change_mode(old_mode, new_mode);
                    }
                    self.cpsr = new_psr;
                }
            }
        }

        CpuAction::AdvancePC(Seq)
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
    pub fn exec_arm_data_processing<
        const OP: u8,
        const IMM: bool,
        const SET_FLAGS: bool,
        const SHIFT_BY_REG: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        use AluOpCode::*;
        let rn = insn.bit_range(16..20) as usize;
        let rd = insn.bit_range(12..16) as usize;
        let mut op1 = if rn == REG_PC {
            self.pc_arm() + 8
        } else {
            self.get_reg(rn)
        };
        let mut s_flag = SET_FLAGS;
        let opcode =
            AluOpCode::from_u8(OP).unwrap_or_else(|| unsafe { std::hint::unreachable_unchecked() });

        let mut carry = self.cpsr.C();
        let op2 = if IMM {
            let immediate = insn & 0xff;
            let rotate = 2 * insn.bit_range(8..12);
            // TODO refactor out
            self.ror(immediate, rotate, &mut carry, false, true)
        } else {
            let reg = insn & 0xf;

            let shift_by = if SHIFT_BY_REG {
                if rn == REG_PC {
                    op1 += 4;
                }
                self.idle_cycle();
                let rs = insn.bit_range(8..12) as usize;
                ShiftRegisterBy::ByRegister(rs)
            } else {
                let amount = insn.bit_range(7..12) as u32;
                ShiftRegisterBy::ByAmount(amount)
            };

            let shifted_reg = ShiftedRegister {
                reg: reg as usize,
                bs_op: insn.get_bs_op(),
                shift_by: shift_by,
                added: None,
            };
            self.register_shift(&shifted_reg, &mut carry)
        };

        if rd == REG_PC && s_flag {
            self.transfer_spsr_mode();
            s_flag = false;
        }

        let alu_res = if s_flag {
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
            let c = carry as u32;
            Some(match opcode {
                AND => op1 & op2,
                EOR => op1 ^ op2,
                SUB => op1.wrapping_sub(op2),
                RSB => op2.wrapping_sub(op1),
                ADD => op1.wrapping_add(op2),
                ADC => op1.wrapping_add(op2).wrapping_add(c),
                SBC => op1.wrapping_sub(op2.wrapping_add(1 - c)),
                RSC => op2.wrapping_sub(op1.wrapping_add(1 - c)),
                ORR => op1 | op2,
                MOV => op2,
                BIC => op1 & (!op2),
                MVN => !op2,
                _ => panic!("DataProcessing should be a PSR transfer"),
            })
        };

        let mut result = CpuAction::AdvancePC(Seq);
        if let Some(alu_res) = alu_res {
            self.set_reg(rd, alu_res as u32);
            if rd == REG_PC {
                // T bit might have changed
                match self.cpsr.state() {
                    CpuState::ARM => self.reload_pipeline32(),
                    CpuState::THUMB => self.reload_pipeline16(),
                };
                result = CpuAction::PipelineFlushed;
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
    pub fn exec_arm_ldr_str<
        const LOAD: bool,
        const WRITEBACK: bool,
        const PRE_INDEX: bool,
        const BYTE: bool,
        const SHIFT: bool,
        const ADD: bool,
        const BS_OP: u8,
        const SHIFT_BY_REG: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let mut result = CpuAction::AdvancePC(NonSeq);

        let base_reg = insn.bit_range(16..20) as usize;
        let dest_reg = insn.bit_range(12..16) as usize;
        let mut addr = self.get_reg(base_reg);
        if base_reg == REG_PC {
            addr = self.pc_arm() + 8; // prefetching
        }
        let mut offset = insn.bit_range(0..12);
        if SHIFT {
            let mut carry = self.cpsr.C();
            let rm = offset & 0xf;
            offset =
                self.register_shift_const::<BS_OP, SHIFT_BY_REG>(offset, rm as usize, &mut carry);
        }
        let offset = if ADD {
            offset as u32
        } else {
            (-(offset as i32)) as u32
        };
        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;

        // TODO - confirm this
        let old_mode = self.cpsr.mode();
        if !PRE_INDEX && WRITEBACK {
            self.change_mode(old_mode, CpuMode::User);
        }

        addr = if PRE_INDEX { effective_addr } else { addr };

        if LOAD {
            let data = if BYTE {
                self.load_8(addr, NonSeq) as u32
            } else {
                self.ldr_word(addr, NonSeq)
            };

            self.set_reg(dest_reg, data);

            // +1I
            self.idle_cycle();

            if dest_reg == REG_PC {
                self.reload_pipeline32();
                result = CpuAction::PipelineFlushed;
            }
        } else {
            let value = if dest_reg == REG_PC {
                self.pc_arm() + 12
            } else {
                self.get_reg(dest_reg)
            };
            if BYTE {
                self.store_8(addr, value as u8, NonSeq);
            } else {
                self.store_aligned_32(addr & !0x3, value, NonSeq);
            };
        }

        if !LOAD || base_reg != dest_reg {
            if !PRE_INDEX {
                self.set_reg(base_reg, effective_addr);
            } else if WRITEBACK {
                self.set_reg(base_reg, effective_addr);
            }
        }

        if !PRE_INDEX && WRITEBACK {
            self.change_mode(self.cpsr.mode(), old_mode);
        }

        result
    }

    pub fn exec_arm_ldr_str_hs_reg<
        const HS: u8,
        const LOAD: bool,
        const WRITEBACK: bool,
        const PRE_INDEX: bool,
        const ADD: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let offset = self.get_reg((insn & 0xf) as usize);
        self.ldr_str_hs_common::<HS, LOAD, WRITEBACK, PRE_INDEX, ADD>(insn, offset)
    }

    pub fn exec_arm_ldr_str_hs_imm<
        const HS: u8,
        const LOAD: bool,
        const WRITEBACK: bool,
        const PRE_INDEX: bool,
        const ADD: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let offset8 = (insn.bit_range(8..12) << 4) + insn.bit_range(0..4);
        self.ldr_str_hs_common::<HS, LOAD, WRITEBACK, PRE_INDEX, ADD>(insn, offset8)
    }

    #[inline(always)]
    pub fn ldr_str_hs_common<
        const HS: u8,
        const LOAD: bool,
        const WRITEBACK: bool,
        const PRE_INDEX: bool,
        const ADD: bool,
    >(
        &mut self,
        insn: u32,
        offset: u32,
    ) -> CpuAction {
        let mut result = CpuAction::AdvancePC(NonSeq);

        let offset = if ADD {
            offset
        } else {
            (-(offset as i32)) as u32
        };
        let base_reg = insn.bit_range(16..20) as usize;
        let dest_reg = insn.bit_range(12..16) as usize;
        let mut addr = self.get_reg(base_reg);
        if base_reg == REG_PC {
            addr = self.pc_arm() + 8; // prefetching
        }

        // TODO - confirm this
        let old_mode = self.cpsr.mode();
        if !PRE_INDEX && WRITEBACK {
            self.change_mode(old_mode, CpuMode::User);
        }

        let effective_addr = (addr as i32).wrapping_add(offset as i32) as Addr;
        addr = if PRE_INDEX { effective_addr } else { addr };

        let transfer_type = ArmHalfwordTransferType::from_u8(HS).unwrap();

        if LOAD {
            let data = match transfer_type {
                ArmHalfwordTransferType::SignedByte => self.load_8(addr, NonSeq) as u8 as i8 as u32,
                ArmHalfwordTransferType::SignedHalfwords => self.ldr_sign_half(addr, NonSeq),
                ArmHalfwordTransferType::UnsignedHalfwords => self.ldr_half(addr, NonSeq),
            };

            self.set_reg(dest_reg, data);

            // +1I
            self.idle_cycle();

            if dest_reg == REG_PC {
                self.reload_pipeline32();
                result = CpuAction::PipelineFlushed;
            }
        } else {
            let value = if dest_reg == REG_PC {
                self.pc_arm() + 12
            } else {
                self.get_reg(dest_reg)
            };

            match transfer_type {
                ArmHalfwordTransferType::UnsignedHalfwords => {
                    self.store_aligned_16(addr, value as u16, NonSeq);
                }
                _ => panic!("invalid HS flags for L=0"),
            };
        }

        if !LOAD || base_reg != dest_reg {
            if !PRE_INDEX {
                self.set_reg(base_reg, effective_addr);
            } else if WRITEBACK {
                self.set_reg(base_reg, effective_addr);
            }
        }

        result
    }

    pub fn exec_arm_ldm_stm<
        const LOAD: bool,
        const WRITEBACK: bool,
        const FLAG_S: bool,
        const ADD: bool,
        const PRE_INDEX: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let mut result = CpuAction::AdvancePC(NonSeq);

        let mut full = PRE_INDEX;
        let ascending = ADD;
        let mut writeback = WRITEBACK;
        let base_reg = insn.bit_range(16..20) as usize;
        let mut base_addr = self.get_reg(base_reg);

        let rlist = insn.register_list();

        if FLAG_S {
            match self.cpsr.mode() {
                CpuMode::User | CpuMode::System => {
                    panic!("LDM/STM with S bit in unprivileged mode")
                }
                _ => {}
            };
        }

        let user_bank_transfer = if FLAG_S {
            if LOAD {
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

        let psr_transfer = FLAG_S & LOAD & rlist.bit(REG_PC);

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
            if LOAD {
                let mut access = NonSeq;
                for r in 0..16 {
                    if rlist.bit(r) {
                        if r == base_reg {
                            writeback = false;
                        }
                        if full {
                            addr = addr.wrapping_add(4);
                        }
                        let val = self.load_32(addr, access);
                        access = Seq;
                        self.set_reg(r, val);
                        if r == REG_PC {
                            if psr_transfer {
                                self.transfer_spsr_mode();
                            }
                            self.reload_pipeline32();
                            result = CpuAction::PipelineFlushed;
                        }
                        if !full {
                            addr = addr.wrapping_add(4);
                        }
                    }
                }
                self.idle_cycle();
            } else {
                let mut first = true;
                let mut access = NonSeq;
                for r in 0..16 {
                    if rlist.bit(r) {
                        let val = if r != base_reg {
                            if r == REG_PC {
                                self.pc_arm() + 12
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

                        first = false;

                        self.store_aligned_32(addr, val, access);
                        access = Seq;
                        if !full {
                            addr = addr.wrapping_add(4);
                        }
                    }
                }
            }
        } else {
            if LOAD {
                let val = self.ldr_word(addr, NonSeq);
                self.set_reg(REG_PC, val & !3);
                self.reload_pipeline32();
                result = CpuAction::PipelineFlushed;
            } else {
                // block data store with empty rlist
                let addr = match (ascending, full) {
                    (false, false) => addr.wrapping_sub(0x3c),
                    (false, true) => addr.wrapping_sub(0x40),
                    (true, false) => addr,
                    (true, true) => addr.wrapping_add(4),
                };
                self.store_aligned_32(addr, self.pc + 4, NonSeq);
            }
            addr = if ascending {
                addr.wrapping_add(0x40)
            } else {
                addr.wrapping_sub(0x40)
            };
        }

        if user_bank_transfer {
            self.change_mode(self.cpsr.mode(), old_mode);
        }

        if writeback {
            self.set_reg(base_reg, addr as u32);
        }

        result
    }

    /// Multiply and Multiply-Accumulate (MUL, MLA)
    /// Execution Time: 1S+mI for MUL, and 1S+(m+1)I for MLA.
    pub fn exec_arm_mul_mla<const UPDATE_FLAGS: bool, const ACCUMULATE: bool>(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let rd = insn.bit_range(16..20) as usize;
        let rn = insn.bit_range(12..16) as usize;
        let rs = insn.bit_range(8..12) as usize;
        let rm = insn.bit_range(0..4) as usize;

        // // check validity
        // assert!(!(REG_PC == rd || REG_PC == rn || REG_PC == rs || REG_PC == rm));
        // assert!(rd != rm);

        let op1 = self.get_reg(rm);
        let op2 = self.get_reg(rs);
        let mut result = op1.wrapping_mul(op2);

        if ACCUMULATE {
            result = result.wrapping_add(self.get_reg(rn));
            self.idle_cycle();
        }

        self.set_reg(rd, result);

        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.idle_cycle();
        }

        if UPDATE_FLAGS {
            self.cpsr.set_N((result as i32) < 0);
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(false);
            self.cpsr.set_V(false);
        }

        CpuAction::AdvancePC(Seq)
    }

    /// Multiply Long and Multiply-Accumulate Long (MULL, MLAL)
    /// Execution Time: 1S+(m+1)I for MULL, and 1S+(m+2)I for MLAL
    pub fn exec_arm_mull_mlal<
        const UPDATE_FLAGS: bool,
        const ACCUMULATE: bool,
        const U_FLAG: bool,
    >(
        &mut self,
        insn: u32,
    ) -> CpuAction {
        let rd_hi = insn.rd_hi();
        let rd_lo = insn.rd_lo();
        let rs = insn.rs();
        let rm = insn.rm();

        let op1 = self.get_reg(rm);
        let op2 = self.get_reg(rs);
        let mut result: u64 = if U_FLAG {
            // signed
            (op1 as i32 as i64).wrapping_mul(op2 as i32 as i64) as u64
        } else {
            (op1 as u64).wrapping_mul(op2 as u64)
        };
        if ACCUMULATE {
            let hi = self.get_reg(rd_hi) as u64;
            let lo = self.get_reg(rd_lo) as u64;
            result = result.wrapping_add(hi << 32 | lo);
            self.idle_cycle();
        }
        self.set_reg(rd_hi, (result >> 32) as i32 as u32);
        self.set_reg(rd_lo, (result & 0xffffffff) as i32 as u32);
        self.idle_cycle();
        let m = self.get_required_multipiler_array_cycles(self.get_reg(rs));
        for _ in 0..m {
            self.idle_cycle();
        }

        if UPDATE_FLAGS {
            self.cpsr.set_N(result.bit(63));
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(false);
            self.cpsr.set_V(false);
        }

        CpuAction::AdvancePC(Seq)
    }

    /// ARM Opcodes: Memory: Single Data Swap (SWP)
    /// Execution Time: 1S+2N+1I. That is, 2N data cycles, 1S code cycle, plus 1I.
    pub fn exec_arm_swp<const BYTE: bool>(&mut self, insn: u32) -> CpuAction {
        let base_addr = self.get_reg(insn.bit_range(16..20) as usize);
        let rd = insn.bit_range(12..16) as usize;
        if BYTE {
            let t = self.load_8(base_addr, NonSeq);
            self.store_8(base_addr, self.get_reg(insn.rm()) as u8, Seq);
            self.set_reg(rd, t as u32);
        } else {
            let t = self.ldr_word(base_addr, NonSeq);
            self.store_aligned_32(base_addr, self.get_reg(insn.rm()), Seq);
            self.set_reg(rd, t as u32);
        }
        self.idle_cycle();

        CpuAction::AdvancePC(NonSeq)
    }

    /// ARM Software Interrupt
    /// Execution Time: 2S+1N
    pub fn exec_arm_swi(&mut self, insn: u32) -> CpuAction {
        self.software_interrupt(self.pc - 4, insn.swi_comment()); // Implies 2S + 1N
        CpuAction::PipelineFlushed
    }
}
