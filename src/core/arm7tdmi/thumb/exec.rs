use crate::core::arm7tdmi::*;
use crate::core::sysbus::SysBus;
use crate::core::Bus;

use crate::bit::BitIndex;

use super::*;
fn push(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    cpu.gpr[REG_SP] -= 4;
    let stack_addr = cpu.gpr[REG_SP] & !3;
    bus.write_32(stack_addr, cpu.get_reg(r))
}
fn pop(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    let val = bus.read_32(cpu.gpr[REG_SP] & !3);
    cpu.set_reg(r, val);
    cpu.gpr[REG_SP] += 4;
}

impl Core {
    /// Format 1
    fn exec_thumb_move_shifted_reg(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        let rd = (insn.raw & 0b111) as usize;
        let rs = insn.raw.bit_range(3..6) as usize;

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

        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 2
    fn exec_thumb_add_sub(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let rd = (insn.raw & 0b111) as usize;
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

        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 3
    fn exec_thumb_data_process_imm(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        use OpFormat3::*;
        let op = insn.format3_op();
        let rd = insn.raw.bit_range(8..11) as usize;
        let op1 = self.gpr[rd];
        let op2_imm = (insn.raw & 0xff) as u32;
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
        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 4
    fn exec_thumb_alu_ops(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let rd = (insn.raw & 0b111) as usize;
        let rs = insn.rs();
        let dst = self.get_reg(rd);
        let src = self.get_reg(rs);

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();

        use ThumbAluOps::*;
        let op = insn.format4_alu_op();
        let result = match op {
            AND | TST => dst & src,
            EOR => dst ^ src,
            LSL | LSR | ASR | ROR => {
                // TODO optimize this second match, keeping it here for code clearity
                let bs_op = match op {
                    LSL => BarrelShiftOpCode::LSL,
                    LSR => BarrelShiftOpCode::LSR,
                    ASR => BarrelShiftOpCode::ASR,
                    ROR => BarrelShiftOpCode::ROR,
                    _ => unreachable!(),
                };
                let result = self.shift_by_register(bs_op, rd, rs, carry);
                carry = self.bs_carry_out;
                result
            }
            ADC => self.alu_adc_flags(dst, src, &mut carry, &mut overflow),
            SBC => self.alu_sbc_flags(dst, src, &mut carry, &mut overflow),
            NEG => self.alu_sub_flags(0, src, &mut carry, &mut overflow),
            CMP => self.alu_sub_flags(dst, src, &mut carry, &mut overflow),
            CMN => self.alu_add_flags(dst, src, &mut carry, &mut overflow),
            ORR => dst | src,
            MUL => {
                let m = self.get_required_multipiler_array_cycles(src);
                for _ in 0..m {
                    self.add_cycle();
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
        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 5
    fn exec_thumb_hi_reg_op_or_bx(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let op = insn.format5_op();
        let rd = (insn.raw & 0b111) as usize;
        let dst_reg = if insn.flag(ThumbInstruction::FLAG_H1) {
            rd + 8
        } else {
            rd
        };
        let src_reg = if insn.flag(ThumbInstruction::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };
        let op1 = self.get_reg(dst_reg);
        let op2 = self.get_reg(src_reg);

        let mut result = CpuAction::AdvancePC;
        match op {
            OpFormat5::BX => {
                return self.branch_exchange(sb, self.get_reg(src_reg));
            }
            OpFormat5::ADD => {
                self.set_reg(dst_reg, op1.wrapping_add(op2));
                if dst_reg == REG_PC {
                    result = CpuAction::FlushPipeline
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
                    result = CpuAction::FlushPipeline;
                }
            }
        }
        self.S_cycle16(sb, self.pc + 2);

        result
    }

    /// Format 6
    fn exec_thumb_ldr_pc(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let rd = insn.raw.bit_range(8..11) as usize;

        let ofs = insn.word8() as Addr;
        let addr = (self.pc & !3) + ofs;

        self.S_cycle16(sb, self.pc + 2);
        let data = self.ldr_word(addr, sb);
        self.N_cycle16(sb, addr);

        self.gpr[rd] = data;

        // +1I
        self.add_cycle();

        CpuAction::AdvancePC
    }

    fn do_exec_thumb_ldr_str(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
        addr: Addr,
        is_transferring_bytes: bool,
    ) -> CpuAction {
        let rd = (insn.raw & 0b111) as usize;
        if insn.is_load() {
            let data = if is_transferring_bytes {
                self.S_cycle8(sb, addr);
                sb.read_8(addr) as u32
            } else {
                self.S_cycle32(sb, addr);
                self.ldr_word(addr, sb)
            };

            self.gpr[rd] = data;

            // +1I
            self.add_cycle();
        } else {
            let value = self.get_reg(rd);
            if is_transferring_bytes {
                self.N_cycle8(sb, addr);
                self.write_8(addr, value as u8, sb);
            } else {
                self.N_cycle32(sb, addr);
                self.write_32(addr, value, sb);
            };
        }

        self.N_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 7
    fn exec_thumb_ldr_str_reg_offset(
        &mut self,
        bus: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        let rb = insn.raw.bit_range(3..6) as usize;
        let addr = self.gpr[rb].wrapping_add(self.gpr[insn.ro()]);
        self.do_exec_thumb_ldr_str(bus, insn, addr, insn.raw.bit(10))
    }

    /// Format 8
    fn exec_thumb_ldr_str_shb(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let rb = insn.raw.bit_range(3..6) as usize;
        let rd = (insn.raw & 0b111) as usize;

        let addr = self.gpr[rb].wrapping_add(self.gpr[insn.ro()]);
        match (
            insn.flag(ThumbInstruction::FLAG_SIGN_EXTEND),
            insn.flag(ThumbInstruction::FLAG_HALFWORD),
        ) {
            (false, false) =>
            /* strh */
            {
                self.write_16(addr, self.gpr[rd] as u16, sb);
                self.N_cycle16(sb, addr);
            }
            (false, true) =>
            /* ldrh */
            {
                self.gpr[rd] = self.ldr_half(addr, sb);
                self.S_cycle16(sb, addr);
                self.add_cycle();
            }
            (true, false) =>
            /* ldsb */
            {
                let val = sb.read_8(addr) as i8 as i32 as u32;
                self.gpr[rd] = val;
                self.S_cycle8(sb, addr);
                self.add_cycle();
            }
            (true, true) =>
            /* ldsh */
            {
                let val = self.ldr_sign_half(addr, sb);
                self.gpr[rd] = val;
                self.S_cycle16(sb, addr);
                self.add_cycle();
            }
        }

        self.N_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 9
    fn exec_thumb_ldr_str_imm_offset(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        let rb = insn.raw.bit_range(3..6) as usize;

        let offset = if insn.raw.bit(12) {
            insn.offset5()
        } else {
            (insn.offset5() << 3) >> 1
        };
        let addr = self.gpr[rb].wrapping_add(offset as u32);
        self.do_exec_thumb_ldr_str(sb, insn, addr, insn.raw.bit(12))
    }

    /// Format 10
    fn exec_thumb_ldr_str_halfword(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        let rb = insn.raw.bit_range(3..6) as usize;
        let rd = (insn.raw & 0b111) as usize;
        let base = self.gpr[rb] as i32;
        let addr = base.wrapping_add((insn.offset5() << 1) as i32) as Addr;
        if insn.is_load() {
            let data = self.ldr_half(addr, sb);
            self.S_cycle16(sb, addr);
            self.add_cycle();
            self.gpr[rd] = data as u32;
        } else {
            self.write_16(addr, self.gpr[rd] as u16, sb);
            self.N_cycle16(sb, addr);
        }
        self.N_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 11
    fn exec_thumb_ldr_str_sp(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let addr = self.gpr[REG_SP] + (insn.word8() as Addr);
        let rd = insn.raw.bit_range(8..11) as usize;
        if insn.is_load() {
            let data = self.ldr_word(addr, sb);
            self.S_cycle16(sb, addr);
            self.add_cycle();
            self.gpr[rd] = data;
        } else {
            self.write_32(addr, self.gpr[rd], sb);
            self.N_cycle16(sb, addr);
        }
        self.N_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 12
    fn exec_thumb_load_address(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let rd = insn.raw.bit_range(8..11) as usize;
        let result = if insn.flag(ThumbInstruction::FLAG_SP) {
            self.gpr[REG_SP] + (insn.word8() as Addr)
        } else {
            (insn.pc & !0b10) + 4 + (insn.word8() as Addr)
        };
        self.gpr[rd] = result;
        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 13
    fn exec_thumb_add_sp(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();

        self.gpr[REG_SP] = op1.wrapping_add(op2) as u32;
        self.S_cycle16(sb, self.pc + 2);

        CpuAction::AdvancePC
    }

    /// Format 14
    fn exec_thumb_push_pop(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let mut result = CpuAction::AdvancePC;

        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).
        let is_pop = insn.is_load();
        let pc_lr_flag = insn.flag(ThumbInstruction::FLAG_R);
        let rlist = insn.register_list();
        self.N_cycle16(sb, self.pc);
        let mut first = true;
        if is_pop {
            for r in 0..8 {
                if rlist.bit(r) {
                    pop(self, sb, r);
                    if first {
                        self.add_cycle();
                        first = false;
                    } else {
                        self.S_cycle16(sb, self.gpr[REG_SP]);
                    }
                }
            }
            if pc_lr_flag {
                pop(self, sb, REG_PC);
                self.pc = self.pc & !1;
                result = CpuAction::FlushPipeline;
            }
            self.S_cycle16(sb, self.pc + 2);
        } else {
            if pc_lr_flag {
                push(self, sb, REG_LR);
            }
            for r in (0..8).rev() {
                if rlist.bit(r) {
                    push(self, sb, r);
                    if first {
                        first = false;
                    } else {
                        self.S_cycle16(sb, self.gpr[REG_SP]);
                    }
                }
            }
        }

        result
    }

    /// Format 15
    fn exec_thumb_ldm_stm(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let mut result = CpuAction::AdvancePC;

        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).

        let rb = insn.raw.bit_range(8..11) as usize;
        let base_reg = rb;
        let is_load = insn.is_load();

        let align_preserve = self.gpr[base_reg] & 3;
        let mut addr = self.gpr[base_reg] & !3;
        let rlist = insn.register_list();
        self.N_cycle16(sb, self.pc);
        let mut first = true;

        if rlist != 0 {
            if is_load {
                let writeback = !rlist.bit(base_reg);
                for r in 0..8 {
                    if rlist.bit(r) {
                        let val = sb.read_32(addr);
                        if first {
                            first = false;
                            self.add_cycle();
                        } else {
                            self.S_cycle16(sb, addr);
                        }
                        addr += 4;
                        self.add_cycle();
                        self.set_reg(r, val);
                    }
                }
                self.S_cycle16(sb, self.pc + 2);
                if writeback {
                    self.gpr[base_reg] = addr + align_preserve;
                }
            } else {
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
                        } else {
                            self.S_cycle16(sb, addr);
                        }
                        sb.write_32(addr, v);
                        addr += 4;
                    }
                    self.gpr[base_reg] = addr + align_preserve;
                }
            }
        } else {
            // From gbatek.htm: Empty Rlist: R15 loaded/stored (ARMv4 only), and Rb=Rb+40h (ARMv4-v5).
            if is_load {
                let val = sb.read_32(addr);
                self.set_reg(REG_PC, val & !1);
                result = CpuAction::FlushPipeline;
            } else {
                sb.write_32(addr, self.pc + 2);
            }
            addr += 0x40;
            self.gpr[base_reg] = addr + align_preserve;
        }

        result
    }

    /// Format 16
    fn exec_thumb_branch_with_cond(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        if !self.check_arm_cond(insn.cond()) {
            self.S_cycle16(sb, self.pc + 2);
            CpuAction::AdvancePC
        } else {
            let offset = insn.bcond_offset();
            self.S_cycle16(sb, self.pc);
            self.pc = (self.pc as i32).wrapping_add(offset) as u32;
            CpuAction::FlushPipeline
        }
    }

    /// Format 17
    fn exec_thumb_swi(&mut self, sb: &mut SysBus, _insn: ThumbInstruction) -> CpuAction {
        self.N_cycle16(sb, self.pc);
        self.exception(sb, Exception::SoftwareInterrupt, self.pc - 2);

        CpuAction::FlushPipeline
    }

    /// Format 18
    fn exec_thumb_branch(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        let offset = ((insn.offset11() << 21) >> 20) as i32;
        self.pc = (self.pc as i32).wrapping_add(offset) as u32;
        self.S_cycle16(sb, self.pc);

        CpuAction::FlushPipeline
    }

    /// Format 19
    fn exec_thumb_branch_long_with_link(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuAction {
        let mut off = insn.offset11();
        if insn.flag(ThumbInstruction::FLAG_LOW_OFFSET) {
            self.S_cycle16(sb, self.pc);
            off = off << 1;
            let next_pc = (self.pc - 2) | 1;
            self.pc = ((self.gpr[REG_LR] & !1) as i32).wrapping_add(off) as u32;
            self.gpr[REG_LR] = next_pc;

            CpuAction::FlushPipeline
        } else {
            off = (off << 21) >> 9;
            self.gpr[REG_LR] = (self.pc as i32).wrapping_add(off) as u32;
            self.S_cycle16(sb, self.pc);

            CpuAction::AdvancePC
        }
    }

    pub fn exec_thumb(&mut self, bus: &mut SysBus, insn: ThumbInstruction) -> CpuAction {
        match insn.fmt {
            ThumbFormat::MoveShiftedReg => self.exec_thumb_move_shifted_reg(bus, insn),
            ThumbFormat::AddSub => self.exec_thumb_add_sub(bus, insn),
            ThumbFormat::DataProcessImm => self.exec_thumb_data_process_imm(bus, insn),
            ThumbFormat::AluOps => self.exec_thumb_alu_ops(bus, insn),
            ThumbFormat::HiRegOpOrBranchExchange => self.exec_thumb_hi_reg_op_or_bx(bus, insn),
            ThumbFormat::LdrPc => self.exec_thumb_ldr_pc(bus, insn),
            ThumbFormat::LdrStrRegOffset => self.exec_thumb_ldr_str_reg_offset(bus, insn),
            ThumbFormat::LdrStrSHB => self.exec_thumb_ldr_str_shb(bus, insn),
            ThumbFormat::LdrStrImmOffset => self.exec_thumb_ldr_str_imm_offset(bus, insn),
            ThumbFormat::LdrStrHalfWord => self.exec_thumb_ldr_str_halfword(bus, insn),
            ThumbFormat::LdrStrSp => self.exec_thumb_ldr_str_sp(bus, insn),
            ThumbFormat::LoadAddress => self.exec_thumb_load_address(bus, insn),
            ThumbFormat::AddSp => self.exec_thumb_add_sp(bus, insn),
            ThumbFormat::PushPop => self.exec_thumb_push_pop(bus, insn),
            ThumbFormat::LdmStm => self.exec_thumb_ldm_stm(bus, insn),
            ThumbFormat::BranchConditional => self.exec_thumb_branch_with_cond(bus, insn),
            ThumbFormat::Swi => self.exec_thumb_swi(bus, insn),
            ThumbFormat::Branch => self.exec_thumb_branch(bus, insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(bus, insn),
        }
    }
}
