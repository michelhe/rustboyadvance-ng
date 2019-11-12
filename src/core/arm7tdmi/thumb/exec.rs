use crate::core::arm7tdmi::alu::AluOpCode;
use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::cpu::{Core, CpuExecResult};
use crate::core::arm7tdmi::*;
use crate::core::sysbus::SysBus;

use crate::bit::BitIndex;

use super::*;
fn push(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    cpu.gpr[REG_SP] -= 4;
    let stack_addr = cpu.gpr[REG_SP] & !3;
    bus.write_32(stack_addr, cpu.get_reg(r))
}
fn pop(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    let stack_addr = cpu.gpr[REG_SP] & !3;
    let val = cpu.ldr_word(stack_addr, bus);
    cpu.set_reg(r, val);
    cpu.gpr[REG_SP] = stack_addr + 4;
}

impl Core {
    fn exec_thumb_move_shifted_reg(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let op2 = self
            .register_shift(ShiftedRegister {
                reg: insn.rs(),
                shift_by: ShiftRegisterBy::ByAmount(insn.offset5() as u8 as u32),
                bs_op: insn.format1_op(),
                added: None,
            })
            .unwrap();

        self.set_reg(insn.rd(), op2);
        self.alu_update_flags(op2, false, self.bs_carry_out, self.cpsr.V());

        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_add_sub(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
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
        self.set_reg(insn.rd(), result as u32);

        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_data_process_imm(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        use OpFormat3::*;
        let op = insn.format3_op();
        let op1 = self.get_reg(insn.rd());
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
            self.set_reg(insn.rd(), result as u32);
        }
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_alu_ops(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let rd = insn.rd();
        let rs = insn.rs();
        let dst = self.get_reg(rd);
        let src = self.get_reg(rs);

        let mut carry = self.cpsr.C();
        let c = self.cpsr.C() as u32;
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
                let shft = BarrelShifterValue::shifted_register(
                    rd,
                    ShiftRegisterBy::ByRegister(rs),
                    bs_op,
                    Some(true),
                );
                let result = self.get_barrel_shifted_value(shft);
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
        Ok(())
    }

    /// Cycles 2S+1N
    fn exec_thumb_bx(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let src_reg = if insn.flag(ThumbInstruction::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };
        self.branch_exchange(sb, self.get_reg(src_reg))
    }

    fn exec_thumb_hi_reg_op_or_bx(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let op = insn.format5_op();
        let dst_reg = if insn.flag(ThumbInstruction::FLAG_H1) {
            insn.rd() + 8
        } else {
            insn.rd()
        };
        let src_reg = if insn.flag(ThumbInstruction::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };
        let op1 = self.get_reg(dst_reg);
        let op2 = self.get_reg(src_reg);

        match op {
            OpFormat5::BX => return self.exec_thumb_bx(sb, insn),
            OpFormat5::ADD => {
                self.set_reg(dst_reg, op1.wrapping_add(op2));
                if dst_reg == REG_PC {
                    self.flush_pipeline(sb);
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
                    self.flush_pipeline(sb);
                }
            }
        }
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_ldr_pc(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (insn.pc & !0b10) + 4 + (insn.word8() as Addr);
        self.S_cycle16(sb, self.pc + 2);
        let data = self.ldr_word(addr, sb);
        self.N_cycle16(sb, addr);

        self.set_reg(insn.rd(), data);
        // +1I
        self.add_cycle();

        Ok(())
    }

    fn do_exec_thumb_ldr_str(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
        addr: Addr,
    ) -> CpuExecResult {
        if insn.is_load() {
            let data = if insn.is_transferring_bytes() {
                self.S_cycle8(sb, addr);
                sb.read_8(addr) as u32
            } else {
                self.S_cycle32(sb, addr);
                self.ldr_word(addr, sb)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();
        } else {
            let value = self.get_reg(insn.rd());
            if insn.is_transferring_bytes() {
                self.N_cycle8(sb, addr);
                self.write_8(addr, value as u8, sb);
            } else {
                self.N_cycle32(sb, addr);
                self.write_32(addr, value, sb);
            };
        }

        self.N_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_ldr_str_reg_offset(
        &mut self,
        bus: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let addr = self
            .get_reg(insn.rb())
            .wrapping_add(self.get_reg(insn.ro()));
        self.do_exec_thumb_ldr_str(bus, insn, addr)
    }

    fn exec_thumb_ldr_str_shb(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = self
            .get_reg(insn.rb())
            .wrapping_add(self.get_reg(insn.ro()));
        let rd = insn.rd();
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
        Ok(())
    }

    fn exec_thumb_ldr_str_imm_offset(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let offset = if insn.raw.bit(12) {
            insn.offset5()
        } else {
            (insn.offset5() << 3) >> 1
        };
        let addr = self.get_reg(insn.rb()).wrapping_add(offset as u32);
        self.do_exec_thumb_ldr_str(sb, insn, addr)
    }

    fn exec_thumb_ldr_str_halfword(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let base = self.gpr[insn.rb()] as i32;
        let addr = base.wrapping_add((insn.offset5() << 1) as i32) as Addr;
        if insn.is_load() {
            let data = self.ldr_half(addr, sb);
            self.S_cycle16(sb, addr);
            self.add_cycle();
            self.gpr[insn.rd()] = data as u32;
        } else {
            self.write_16(addr, self.gpr[insn.rd()] as u16, sb);
            self.N_cycle16(sb, addr);
        }
        self.N_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_ldr_str_sp(&mut self, bus: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = self.gpr[REG_SP] + (insn.word8() as Addr);
        self.do_exec_thumb_ldr_str_with_addr(bus, insn, addr)
    }

    fn exec_thumb_load_address(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let result = if insn.flag(ThumbInstruction::FLAG_SP) {
            self.gpr[REG_SP] + (insn.word8() as Addr)
        } else {
            (insn.pc & !0b10) + 4 + (insn.word8() as Addr)
        };
        self.gpr[insn.rd()] = result;
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn do_exec_thumb_ldr_str_with_addr(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
        addr: Addr,
    ) -> CpuExecResult {
        if insn.is_load() {
            let data = self.ldr_word(addr, sb);
            self.S_cycle16(sb, addr);
            self.add_cycle();
            self.gpr[insn.rd()] = data;
        } else {
            self.write_32(addr, self.gpr[insn.rd()], sb);
            self.N_cycle16(sb, addr);
        }
        self.N_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_add_sp(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();

        self.gpr[REG_SP] = op1.wrapping_add(op2) as u32;
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_push_pop(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
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
                self.flush_pipeline(sb);
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

        Ok(())
    }

    fn exec_thumb_ldm_stm(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).

        let is_load = insn.is_load();
        let rb = insn.rb();

        let mut addr = self.gpr[rb] & !3;
        let rlist = insn.register_list();
        self.N_cycle16(sb, self.pc);
        let mut first = true;
        let mut writeback = true;

        if rlist != 0 {
            if is_load {
                for r in 0..8 {
                    if rlist.bit(r) {
                        if r == rb {
                            writeback = false;
                        }
                        let val = self.ldr_word(addr, sb);
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
            } else {
                for r in 0..8 {
                    if rlist.bit(r) {
                        if first {
                            first = false;
                        } else {
                            self.S_cycle16(sb, addr);
                        }
                        self.write_32(addr, self.gpr[r], sb);
                        addr += 4;
                    }
                }
            }
        } else {
            // From gbatek.htm: Empty Rlist: R15 loaded/stored (ARMv4 only), and Rb=Rb+40h (ARMv4-v5).
            if is_load {
                let val = self.ldr_word(addr, sb);
                self.set_reg(REG_PC, val & !1);
                self.flush_pipeline(sb);
            } else {
                self.write_32(addr, self.pc + 2, sb);
            }
            addr += 0x40;
        }

        if writeback {
            self.gpr[rb] = addr;
        }

        Ok(())
    }

    fn exec_thumb_branch_with_cond(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond()) {
            self.S_cycle16(sb, self.pc + 2);
            Ok(())
        } else {
            let offset = insn.bcond_offset();
            self.S_cycle16(sb, self.pc);
            self.pc = (self.pc as i32).wrapping_add(offset) as u32;
            self.flush_pipeline(sb);
            Ok(())
        }
    }

    fn exec_thumb_branch(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let offset = ((insn.offset11() << 21) >> 20) as i32;
        self.pc = (self.pc as i32).wrapping_add(offset) as u32;
        self.S_cycle16(sb, self.pc);
        self.flush_pipeline(sb);
        Ok(())
    }

    fn exec_thumb_branch_long_with_link(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let mut off = insn.offset11();
        if insn.flag(ThumbInstruction::FLAG_LOW_OFFSET) {
            self.S_cycle16(sb, self.pc);
            off = off << 1;
            let next_pc = (self.pc - 2) | 1;
            self.pc = ((self.gpr[REG_LR] & !1) as i32).wrapping_add(off) as u32;
            self.gpr[REG_LR] = next_pc;

            self.flush_pipeline(sb);
            Ok(())
        } else {
            off = (off << 21) >> 9;
            self.gpr[REG_LR] = (self.pc as i32).wrapping_add(off) as u32;
            self.S_cycle16(sb, self.pc);
            Ok(())
        }
    }

    pub fn exec_thumb(&mut self, bus: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
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
            ThumbFormat::Swi => {
                self.software_interrupt(bus, insn.pc + 2, (insn.raw & 0xff) as u32);
                Ok(())
            }
            ThumbFormat::Branch => self.exec_thumb_branch(bus, insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(bus, insn),
        }
    }
}
