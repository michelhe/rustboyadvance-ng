use crate::arm7tdmi::*;

use crate::bit::BitIndex;

use super::super::memory::{MemoryAccess, MemoryInterface};
use super::ThumbDecodeHelper;
use super::*;
use MemoryAccess::*;

impl<I: MemoryInterface> Core<I> {
    /// Format 1
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_move_shifted_reg<const BS_OP: u8, const IMM: u8>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let rs = insn.bit_range(3..6) as usize;

        let shift_amount = IMM as u32;
        let mut carry = self.cpsr.C();
        let bsop = match BS_OP {
            0 => BarrelShiftOpCode::LSL,
            1 => BarrelShiftOpCode::LSR,
            2 => BarrelShiftOpCode::ASR,
            3 => BarrelShiftOpCode::ROR,
            _ => unsafe { std::hint::unreachable_unchecked() },
        };
        let op2 = self.barrel_shift_op(bsop, self.gpr[rs], shift_amount, &mut carry, true);
        self.gpr[rd] = op2;
        self.alu_update_flags(op2, false, carry, self.cpsr.V());

        CpuAction::AdvancePC(Seq)
    }

    /// Format 2
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_add_sub<
        const SUB: bool,
        const IMM: bool,
        const RN: usize,
    >(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let op1 = self.get_reg(insn.rs());
        let op2 = if IMM { RN as u32 } else { self.get_reg(RN) };

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();
        let result = if SUB {
            self.alu_sub_flags(op1, op2, &mut carry, &mut overflow)
        } else {
            self.alu_add_flags(op1, op2, &mut carry, &mut overflow)
        };
        self.alu_update_flags(result, true, carry, overflow);
        self.set_reg(rd, result as u32);

        CpuAction::AdvancePC(Seq)
    }

    /// Format 3
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_data_process_imm<const OP: u8, const RD: usize>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        use OpFormat3::*;
        let op = OpFormat3::from_u8(OP).unwrap();
        let op1 = self.gpr[RD];
        let op2_imm = (insn & 0xff) as u32;
        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();
        let result = match op {
            MOV => op2_imm,
            CMP | SUB => self.alu_sub_flags(op1, op2_imm, &mut carry, &mut overflow),
            ADD => self.alu_add_flags(op1, op2_imm, &mut carry, &mut overflow),
        };
        let arithmetic = op == ADD || op == SUB;
        self.alu_update_flags(result, arithmetic, carry, overflow);
        if op != CMP {
            self.gpr[RD] = result as u32;
        }

        CpuAction::AdvancePC(Seq)
    }

    /// Format 4
    /// Execution Time:
    ///     1S      for  AND,EOR,ADC,SBC,TST,NEG,CMP,CMN,ORR,BIC,MVN
    ///     1S+1I   for  LSL,LSR,ASR,ROR
    ///     1S+mI   for  MUL on ARMv4 (m=1..4; depending on MSBs of incoming Rd value)
    pub(in super::super) fn exec_thumb_alu_ops<const OP: u16>(&mut self, insn: u16) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let rs = insn.rs();
        let dst = self.get_reg(rd);
        let src = self.get_reg(rs);

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();

        use ThumbAluOps::*;
        let op = ThumbAluOps::from_u16(OP).unwrap();

        macro_rules! shifter_op {
            ($bs_op:expr) => {{
                let result = self.shift_by_register($bs_op, rd, rs, &mut carry);
                self.idle_cycle();
                result
            }};
        }

        let result = match op {
            AND | TST => dst & src,
            EOR => dst ^ src,
            LSL => shifter_op!(BarrelShiftOpCode::LSL),
            LSR => shifter_op!(BarrelShiftOpCode::LSR),
            ASR => shifter_op!(BarrelShiftOpCode::ASR),
            ROR => shifter_op!(BarrelShiftOpCode::ROR),
            ADC => self.alu_adc_flags(dst, src, &mut carry, &mut overflow),
            SBC => self.alu_sbc_flags(dst, src, &mut carry, &mut overflow),
            NEG => self.alu_sub_flags(0, src, &mut carry, &mut overflow),
            CMP => self.alu_sub_flags(dst, src, &mut carry, &mut overflow),
            CMN => self.alu_add_flags(dst, src, &mut carry, &mut overflow),
            ORR => dst | src,
            MUL => {
                let m = self.get_required_multipiler_array_cycles(src);
                for _ in 0..m {
                    self.idle_cycle();
                }
                // TODO - meaningless values?
                carry = false;
                overflow = false;
                dst.wrapping_mul(src)
            }
            BIC => dst & (!src),
            MVN => !src,
        };
        self.alu_update_flags(result, op.is_arithmetic(), carry, overflow);

        if !op.is_setting_flags() {
            self.set_reg(rd, result as u32);
        }

        CpuAction::AdvancePC(Seq)
    }

    /// Format 5
    /// Execution Time:
    ///     1S     for ADD/MOV/CMP
    ///     2S+1N  for ADD/MOV with Rd=R15, and for BX
    pub(in super::super) fn exec_thumb_hi_reg_op_or_bx<
        const OP: u8,
        const FLAG_H1: bool,
        const FLAG_H2: bool,
    >(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let op = OpFormat5::from_u8(OP).unwrap();
        let rd = (insn & 0b111) as usize;
        let rs = insn.rs();
        let dst_reg = if FLAG_H1 { rd + 8 } else { rd };
        let src_reg = if FLAG_H2 { rs + 8 } else { rs };
        let op1 = self.get_reg(dst_reg);
        let op2 = self.get_reg(src_reg);

        let mut result = CpuAction::AdvancePC(Seq);
        match op {
            OpFormat5::BX => {
                return self.branch_exchange(self.get_reg(src_reg));
            }
            OpFormat5::ADD => {
                self.set_reg(dst_reg, op1.wrapping_add(op2));
                if dst_reg == REG_PC {
                    self.reload_pipeline16();
                    result = CpuAction::PipelineFlushed;
                }
            }
            OpFormat5::CMP => {
                let mut carry = self.cpsr.C();
                let mut overflow = self.cpsr.V();
                let result = self.alu_sub_flags(op1, op2, &mut carry, &mut overflow);
                self.alu_update_flags(result, true, carry, overflow);
            }
            OpFormat5::MOV => {
                self.set_reg(dst_reg, op2 as u32);
                if dst_reg == REG_PC {
                    self.reload_pipeline16();
                    result = CpuAction::PipelineFlushed;
                }
            }
        }

        result
    }

    /// Format 6 load PC-relative (for loading immediates from literal pool)
    /// Execution Time: 1S+1N+1I
    pub(in super::super) fn exec_thumb_ldr_pc<const RD: usize>(&mut self, insn: u16) -> CpuAction {
        let ofs = insn.word8() as Addr;
        let addr = (self.pc & !3) + ofs;

        self.gpr[RD] = self.load_32(addr, NonSeq);

        // +1I
        self.idle_cycle();

        CpuAction::AdvancePC(NonSeq)
    }

    /// Helper function for various ldr/str handler
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    fn do_exec_thumb_ldr_str<const LOAD: bool, const BYTE: bool>(
        &mut self,
        insn: u16,
        addr: Addr,
    ) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        if LOAD {
            let data = if BYTE {
                self.load_8(addr, NonSeq) as u32
            } else {
                self.ldr_word(addr, NonSeq)
            };

            self.gpr[rd] = data;

            // +1I
            self.idle_cycle();
            CpuAction::AdvancePC(Seq)
        } else {
            let value = self.get_reg(rd);
            if BYTE {
                self.store_8(addr, value as u8, NonSeq);
            } else {
                self.store_aligned_32(addr, value, NonSeq);
            };
            CpuAction::AdvancePC(NonSeq)
        }
    }

    /// Format 7 load/store with register offset
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_reg_offset<
        const LOAD: bool,
        const RO: usize,
        const BYTE: bool,
    >(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let addr = self.gpr[rb].wrapping_add(self.gpr[RO]);
        self.do_exec_thumb_ldr_str::<LOAD, BYTE>(insn, addr)
    }

    /// Format 8 load/store sign-extended byte/halfword
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_shb<
        const RO: usize,
        const SIGN_EXTEND: bool,
        const HALFWORD: bool,
    >(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let rd = (insn & 0b111) as usize;

        let addr = self.gpr[rb].wrapping_add(self.gpr[RO]);
        match (SIGN_EXTEND, HALFWORD) {
            (false, false) =>
            /* strh */
            {
                self.store_aligned_16(addr, self.gpr[rd] as u16, NonSeq);
            }
            (false, true) =>
            /* ldrh */
            {
                self.gpr[rd] = self.ldr_half(addr, NonSeq);
                self.idle_cycle();
            }
            (true, false) =>
            /* ldself */
            {
                let val = self.load_8(addr, NonSeq) as i8 as i32 as u32;
                self.gpr[rd] = val;
                self.idle_cycle();
            }
            (true, true) =>
            /* ldsh */
            {
                let val = self.ldr_sign_half(addr, NonSeq);
                self.gpr[rd] = val;
                self.idle_cycle();
            }
        }

        CpuAction::AdvancePC(NonSeq)
    }

    /// Format 9
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_imm_offset<
        const LOAD: bool,
        const BYTE: bool,
        const OFFSET: u8,
    >(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let addr = self.gpr[rb].wrapping_add(OFFSET as u32);
        self.do_exec_thumb_ldr_str::<LOAD, BYTE>(insn, addr)
    }

    /// Format 10
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_halfword<const LOAD: bool, const OFFSET: i32>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let rd = (insn & 0b111) as usize;
        let base = self.gpr[rb] as i32;
        let addr = base.wrapping_add(OFFSET) as Addr;
        if LOAD {
            let data = self.ldr_half(addr, NonSeq);
            self.idle_cycle();
            self.gpr[rd] = data as u32;
            CpuAction::AdvancePC(Seq)
        } else {
            self.store_aligned_16(addr, self.gpr[rd] as u16, NonSeq);
            CpuAction::AdvancePC(NonSeq)
        }
    }

    /// Format 11 load/store SP-relative
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_sp<const LOAD: bool, const RD: usize>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let addr = self.gpr[REG_SP] + (insn.word8() as Addr);
        if LOAD {
            let data = self.ldr_word(addr, NonSeq);
            self.idle_cycle();
            self.gpr[RD] = data;
            CpuAction::AdvancePC(Seq)
        } else {
            self.store_aligned_32(addr, self.gpr[RD], NonSeq);
            CpuAction::AdvancePC(NonSeq)
        }
    }

    /// Format 12
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_load_address<const SP: bool, const RD: usize>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        self.gpr[RD] = if SP {
            self.gpr[REG_SP] + (insn.word8() as Addr)
        } else {
            (self.pc_thumb() & !0b10) + 4 + (insn.word8() as Addr)
        };

        CpuAction::AdvancePC(Seq)
    }

    /// Format 13
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_add_sp<const FLAG_S: bool>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let op1 = self.gpr[REG_SP] as i32;
        let offset = ((insn & 0x7f) << 2) as i32;
        self.gpr[REG_SP] = if FLAG_S {
            op1.wrapping_sub(offset) as u32
        } else {
            op1.wrapping_add(offset) as u32
        };

        CpuAction::AdvancePC(Seq)
    }
    /// Format 14
    /// Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).
    pub(in super::super) fn exec_thumb_push_pop<const POP: bool, const FLAG_R: bool>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        macro_rules! push {
            ($r:expr, $access:ident) => {
                self.gpr[REG_SP] -= 4;
                let stack_addr = self.gpr[REG_SP] & !3;
                self.store_32(stack_addr, self.get_reg($r), $access);
                $access = Seq;
            };
        }
        macro_rules! pop {
            ($r:expr) => {
                let val = self.load_32(self.gpr[REG_SP] & !3, Seq);
                self.set_reg($r, val);
                self.gpr[REG_SP] += 4;
            };
            ($r:expr, $access:ident) => {
                let val = self.load_32(self.gpr[REG_SP] & !3, $access);
                $access = Seq;
                self.set_reg($r, val);
                self.gpr[REG_SP] += 4;
            };
        }
        let mut result = CpuAction::AdvancePC(NonSeq);
        let rlist = insn.register_list();
        let mut access = MemoryAccess::NonSeq;
        if POP {
            for r in 0..8 {
                if rlist.bit(r) {
                    pop!(r, access);
                }
            }
            if FLAG_R {
                pop!(REG_PC);
                self.pc = self.pc & !1;
                result = CpuAction::PipelineFlushed;
                self.reload_pipeline16();
            }
            // Idle 1 cycle
            self.idle_cycle();
        } else {
            if FLAG_R {
                push!(REG_LR, access);
            }
            for r in (0..8).rev() {
                if rlist.bit(r) {
                    push!(r, access);
                }
            }
        }

        result
    }

    /// Format 15
    /// Execution Time: nS+1N+1I for LDM, or (n-1)S+2N for STM.
    pub(in super::super) fn exec_thumb_ldm_stm<const LOAD: bool, const RB: usize>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let mut result = CpuAction::AdvancePC(NonSeq);

        let align_preserve = self.gpr[RB] & 3;
        let mut addr = self.gpr[RB] & !3;
        let rlist = insn.register_list();
        // let mut first = true;
        if rlist != 0 {
            if LOAD {
                let mut access = NonSeq;
                for r in 0..8 {
                    if rlist.bit(r) {
                        let val = self.load_32(addr, access);
                        access = Seq;
                        addr += 4;
                        self.set_reg(r, val);
                    }
                }
                self.idle_cycle();
                if !rlist.bit(RB) {
                    self.gpr[RB] = addr + align_preserve;
                }
            } else {
                let mut first = true;
                let mut access = NonSeq;
                for r in 0..8 {
                    if rlist.bit(r) {
                        let v = if r != RB {
                            self.gpr[r]
                        } else {
                            if first {
                                addr
                            } else {
                                addr + (rlist.count_ones() - 1) * 4
                            }
                        };
                        if first {
                            first = false;
                        }
                        self.store_32(addr, v, access);
                        access = Seq;
                        addr += 4;
                    }
                    self.gpr[RB] = addr + align_preserve;
                }
            }
        } else {
            // From gbatek.htm: Empty Rlist: R15 loaded/stored (ARMv4 only), and Rb=Rb+40h (ARMv4-v5).
            if LOAD {
                let val = self.load_32(addr, NonSeq);
                self.pc = val & !1;
                result = CpuAction::PipelineFlushed;
                self.reload_pipeline16();
            } else {
                self.store_32(addr, self.pc + 2, NonSeq);
            }
            addr += 0x40;
            self.gpr[RB] = addr + align_preserve;
        }

        result
    }

    /// Format 16
    /// Execution Time:
    ///     2S+1N   if condition true (jump executed)
    ///     1S      if condition false
    pub(in super::super) fn exec_thumb_branch_with_cond<const COND: u8>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let cond = ArmCond::from_u8(COND).expect("bad cond");
        if !self.check_arm_cond(cond) {
            CpuAction::AdvancePC(Seq)
        } else {
            let offset = insn.bcond_offset();
            self.pc = (self.pc as i32).wrapping_add(offset) as u32;
            self.reload_pipeline16();
            CpuAction::PipelineFlushed
        }
    }

    /// Format 17
    /// Execution Time: 2S+1N
    pub(in super::super) fn exec_thumb_swi(&mut self, _insn: u16) -> CpuAction {
        self.exception(Exception::SoftwareInterrupt, self.pc - 2); // implies pipeline reload
        CpuAction::PipelineFlushed
    }

    /// Format 18
    /// Execution Time: 2S+1N
    pub(in super::super) fn exec_thumb_branch(&mut self, insn: u16) -> CpuAction {
        let offset = ((insn.offset11() << 21) >> 20) as i32;
        self.pc = (self.pc as i32).wrapping_add(offset) as u32;
        self.reload_pipeline16(); // 2S + 1N
        CpuAction::PipelineFlushed
    }

    /// Format 19
    /// Execution Time: 3S+1N (first opcode 1S, second opcode 2S+1N).
    pub(in super::super) fn exec_thumb_branch_long_with_link<const FLAG_LOW_OFFSET: bool>(
        &mut self,
        insn: u16,
    ) -> CpuAction {
        let mut off = insn.offset11();
        if FLAG_LOW_OFFSET {
            off = off << 1;
            let next_pc = (self.pc - 2) | 1;
            self.pc = ((self.gpr[REG_LR] & !1) as i32).wrapping_add(off) as u32;
            self.gpr[REG_LR] = next_pc;
            self.reload_pipeline16(); // implies 2S + 1N
            CpuAction::PipelineFlushed
        } else {
            off = (off << 21) >> 9;
            self.gpr[REG_LR] = (self.pc as i32).wrapping_add(off) as u32;
            CpuAction::AdvancePC(Seq) // 1S
        }
    }

    pub fn thumb_undefined(&mut self, insn: u16) -> CpuAction {
        panic!(
            "executing undefind thumb instruction {:04x} at @{:08x}",
            insn,
            self.pc_thumb()
        )
    }
}
