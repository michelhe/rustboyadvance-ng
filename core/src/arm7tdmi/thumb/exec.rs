use crate::arm7tdmi::*;

use crate::bit::BitIndex;

use super::super::memory::{MemoryAccess, MemoryInterface};
use super::ThumbDecodeHelper;
use super::*;
use MemoryAccess::*;

impl<I: MemoryInterface> Core<I> {
    /// Format 1
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_move_shifted_reg(&mut self, insn: u16) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let rs = insn.bit_range(3..6) as usize;

        let shift_amount = insn.offset5() as u8 as u32;
        let op2 = self.barrel_shift_op(
            insn.format1_op(),
            self.gpr[rs],
            shift_amount,
            self.cpsr.C(),
            true,
        );
        self.gpr[rd] = op2;
        self.alu_update_flags(op2, false, self.bs_carry_out, self.cpsr.V());

        CpuAction::AdvancePC(Seq)
    }

    /// Format 2
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_add_sub(&mut self, insn: u16) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let op1 = self.get_reg(insn.rs());
        let op2 = if insn.is_immediate_operand() {
            insn.rn() as u32
        } else {
            self.get_reg(insn.rn())
        };

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();
        let result = if insn.is_subtract() {
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
    pub(in super::super) fn exec_thumb_data_process_imm(&mut self, insn: u16) -> CpuAction {
        use OpFormat3::*;
        let op = insn.format3_op();
        let rd = insn.bit_range(8..11) as usize;
        let op1 = self.gpr[rd];
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
            self.gpr[rd] = result as u32;
        }

        CpuAction::AdvancePC(Seq)
    }

    /// Format 4
    /// Execution Time:
    ///     1S      for  AND,EOR,ADC,SBC,TST,NEG,CMP,CMN,ORR,BIC,MVN
    ///     1S+1I   for  LSL,LSR,ASR,ROR
    ///     1S+mI   for  MUL on ARMv4 (m=1..4; depending on MSBs of incoming Rd value)
    pub(in super::super) fn exec_thumb_alu_ops(&mut self, insn: u16) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        let rs = insn.rs();
        let dst = self.get_reg(rd);
        let src = self.get_reg(rs);

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();

        use ThumbAluOps::*;
        let op = insn.format4_alu_op();

        macro_rules! shifter_op {
            ($bs_op:expr) => {{
                let result = self.shift_by_register($bs_op, rd, rs, carry);
                self.idle_cycle();
                carry = self.bs_carry_out;
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
    pub(in super::super) fn exec_thumb_hi_reg_op_or_bx(&mut self, insn: u16) -> CpuAction {
        let op = insn.format5_op();
        let rd = (insn & 0b111) as usize;
        let dst_reg = if insn.bit(consts::flags::FLAG_H1) {
            rd + 8
        } else {
            rd
        };
        let src_reg = if insn.bit(consts::flags::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };
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
    pub(in super::super) fn exec_thumb_ldr_pc(&mut self, insn: u16) -> CpuAction {
        let rd = insn.bit_range(8..11) as usize;

        let ofs = insn.word8() as Addr;
        let addr = (self.pc & !3) + ofs;

        self.gpr[rd] = self.load_32(addr, NonSeq);

        // +1I
        self.idle_cycle();

        CpuAction::AdvancePC(NonSeq)
    }

    /// Helper function for various ldr/str handler
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    fn do_exec_thumb_ldr_str(
        &mut self,
        insn: u16,

        addr: Addr,
        is_transferring_bytes: bool,
    ) -> CpuAction {
        let rd = (insn & 0b111) as usize;
        if insn.is_load() {
            let data = if is_transferring_bytes {
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
            if is_transferring_bytes {
                self.store_8(addr, value as u8, NonSeq);
            } else {
                self.store_aligned_32(addr, value, NonSeq);
            };
            CpuAction::AdvancePC(NonSeq)
        }
    }

    /// Format 7 load/store with register offset
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_reg_offset(&mut self, insn: u16) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let addr = self.gpr[rb].wrapping_add(self.gpr[insn.ro()]);
        self.do_exec_thumb_ldr_str(insn, addr, insn.bit(10))
    }

    /// Format 8 load/store sign-extended byte/halfword
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_shb(&mut self, insn: u16) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let rd = (insn & 0b111) as usize;

        let addr = self.gpr[rb].wrapping_add(self.gpr[insn.ro()]);
        match (
            insn.bit(consts::flags::FLAG_SIGN_EXTEND),
            insn.bit(consts::flags::FLAG_HALFWORD),
        ) {
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
    pub(in super::super) fn exec_thumb_ldr_str_imm_offset(&mut self, insn: u16) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;

        let offset = if insn.bit(12) {
            insn.offset5()
        } else {
            (insn.offset5() << 3) >> 1
        };
        let addr = self.gpr[rb].wrapping_add(offset as u32);
        self.do_exec_thumb_ldr_str(insn, addr, insn.bit(12))
    }

    /// Format 10
    /// Execution Time: 1S+1N+1I for LDR, or 2N for STR
    pub(in super::super) fn exec_thumb_ldr_str_halfword(&mut self, insn: u16) -> CpuAction {
        let rb = insn.bit_range(3..6) as usize;
        let rd = (insn & 0b111) as usize;
        let base = self.gpr[rb] as i32;
        let addr = base.wrapping_add((insn.offset5() << 1) as i32) as Addr;
        if insn.is_load() {
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
    pub(in super::super) fn exec_thumb_ldr_str_sp(&mut self, insn: u16) -> CpuAction {
        let addr = self.gpr[REG_SP] + (insn.word8() as Addr);
        let rd = insn.bit_range(8..11) as usize;
        if insn.is_load() {
            let data = self.ldr_word(addr, NonSeq);
            self.idle_cycle();
            self.gpr[rd] = data;
            CpuAction::AdvancePC(Seq)
        } else {
            self.store_aligned_32(addr, self.gpr[rd], NonSeq);
            CpuAction::AdvancePC(NonSeq)
        }
    }

    /// Format 12
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_load_address(&mut self, insn: u16) -> CpuAction {
        let rd = insn.bit_range(8..11) as usize;

        self.gpr[rd] = if insn.bit(consts::flags::FLAG_SP) {
            self.gpr[REG_SP] + (insn.word8() as Addr)
        } else {
            (self.pc_thumb() & !0b10) + 4 + (insn.word8() as Addr)
        };

        CpuAction::AdvancePC(Seq)
    }

    /// Format 13
    /// Execution Time: 1S
    pub(in super::super) fn exec_thumb_add_sp(&mut self, insn: u16) -> CpuAction {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();

        self.gpr[REG_SP] = op1.wrapping_add(op2) as u32;

        CpuAction::AdvancePC(Seq)
    }
    /// Format 14
    /// Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).
    pub(in super::super) fn exec_thumb_push_pop(&mut self, insn: u16) -> CpuAction {
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
        let is_pop = insn.is_load();
        let pc_lr_flag = insn.bit(consts::flags::FLAG_R);
        let rlist = insn.register_list();
        let mut access = MemoryAccess::NonSeq;
        if is_pop {
            for r in 0..8 {
                if rlist.bit(r) {
                    pop!(r, access);
                }
            }
            if pc_lr_flag {
                pop!(REG_PC);
                self.pc = self.pc & !1;
                result = CpuAction::PipelineFlushed;
                self.reload_pipeline16();
            }
            // Idle 1 cycle
            self.idle_cycle();
        } else {
            if pc_lr_flag {
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
    pub(in super::super) fn exec_thumb_ldm_stm(&mut self, insn: u16) -> CpuAction {
        let mut result = CpuAction::AdvancePC(NonSeq);

        let rb = insn.bit_range(8..11) as usize;
        let base_reg = rb;
        let is_load = insn.is_load();

        let align_preserve = self.gpr[base_reg] & 3;
        let mut addr = self.gpr[base_reg] & !3;
        let rlist = insn.register_list();
        // let mut first = true;
        if rlist != 0 {
            if is_load {
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
                if !rlist.bit(base_reg) {
                    self.gpr[base_reg] = addr + align_preserve;
                }
            } else {
                let mut first = true;
                let mut access = NonSeq;
                for r in 0..8 {
                    if rlist.bit(r) {
                        let v = if r != base_reg {
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
                    self.gpr[base_reg] = addr + align_preserve;
                }
            }
        } else {
            // From gbatek.htm: Empty Rlist: R15 loaded/stored (ARMv4 only), and Rb=Rb+40h (ARMv4-v5).
            if is_load {
                let val = self.load_32(addr, NonSeq);
                self.pc = val & !1;
                result = CpuAction::PipelineFlushed;
                self.reload_pipeline16();
            } else {
                self.store_32(addr, self.pc + 2, NonSeq);
            }
            addr += 0x40;
            self.gpr[base_reg] = addr + align_preserve;
        }

        result
    }

    /// Format 16
    /// Execution Time:
    ///     2S+1N   if condition true (jump executed)
    ///     1S      if condition false
    pub(in super::super) fn exec_thumb_branch_with_cond(&mut self, insn: u16) -> CpuAction {
        if !self.check_arm_cond(insn.cond()) {
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
    pub(in super::super) fn exec_thumb_branch_long_with_link(&mut self, insn: u16) -> CpuAction {
        let mut off = insn.offset11();
        if insn.bit(consts::flags::FLAG_LOW_OFFSET) {
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

    #[cfg(not(feature = "arm7tdmi_dispatch_table"))]
    pub fn exec_thumb(&mut self, insn: u16, fmt: ThumbFormat) -> CpuAction {
        match fmt {
            ThumbFormat::MoveShiftedReg => self.exec_thumb_move_shifted_reg(insn),
            ThumbFormat::AddSub => self.exec_thumb_add_sub(insn),
            ThumbFormat::DataProcessImm => self.exec_thumb_data_process_imm(insn),
            ThumbFormat::AluOps => self.exec_thumb_alu_ops(insn),
            ThumbFormat::HiRegOpOrBranchExchange => self.exec_thumb_hi_reg_op_or_bx(insn),
            ThumbFormat::LdrPc => self.exec_thumb_ldr_pc(insn),
            ThumbFormat::LdrStrRegOffset => self.exec_thumb_ldr_str_reg_offset(insn),
            ThumbFormat::LdrStrSHB => self.exec_thumb_ldr_str_shb(insn),
            ThumbFormat::LdrStrImmOffset => self.exec_thumb_ldr_str_imm_offset(insn),
            ThumbFormat::LdrStrHalfWord => self.exec_thumb_ldr_str_halfword(insn),
            ThumbFormat::LdrStrSp => self.exec_thumb_ldr_str_sp(insn),
            ThumbFormat::LoadAddress => self.exec_thumb_load_address(insn),
            ThumbFormat::AddSp => self.exec_thumb_add_sp(insn),
            ThumbFormat::PushPop => self.exec_thumb_push_pop(insn),
            ThumbFormat::LdmStm => self.exec_thumb_ldm_stm(insn),
            ThumbFormat::BranchConditional => self.exec_thumb_branch_with_cond(insn),
            ThumbFormat::Swi => self.exec_thumb_swi(insn),
            ThumbFormat::Branch => self.exec_thumb_branch(insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(insn),
            ThumbFormat::Undefined => self.thumb_undefined(insn),
        }
    }
}
