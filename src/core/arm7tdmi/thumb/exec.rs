use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::cpu::{Core, CpuExecResult};
use crate::core::arm7tdmi::alu::AluOpCode;
use crate::core::arm7tdmi::*;
use crate::core::sysbus::SysBus;

use super::*;
fn push(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    cpu.gpr[REG_SP] -= 4;
    let stack_addr = cpu.gpr[REG_SP];
    bus.write_32(stack_addr, cpu.get_reg(r))
}
fn pop(cpu: &mut Core, bus: &mut SysBus, r: usize) {
    let stack_addr = cpu.gpr[REG_SP];
    let val = bus.read_32(stack_addr);
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
            .unwrap() as i32;
        self.cpsr.set_C(self.bs_carry_out);

        let rd = insn.rd();
        let op1 = self.get_reg(rd) as i32;
        let result = self.alu_flags(AluOpCode::MOV, op1, op2);
        if let Some(result) = result {
            self.set_reg(rd, result as u32);
        }
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_add_sub(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.get_reg(insn.rs()) as i32;
        let op2 = if insn.is_immediate_operand() {
            insn.rn() as u32 as i32
        } else {
            self.get_reg(insn.rn()) as i32
        };
        let arm_alu_op = if insn.is_subtract() {
            AluOpCode::SUB
        } else {
            AluOpCode::ADD
        };

        let result = self.alu_flags(arm_alu_op, op1, op2);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_data_process_imm(
        &mut self,
        sb: &mut SysBus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let arm_alu_op: AluOpCode = insn.format3_op().into();
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = ((insn.raw & 0xff) as i8) as u8 as i32;
        let result = self.alu_flags(arm_alu_op, op1, op2);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_mul(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = self.get_reg(insn.rs()) as i32;
        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.add_cycle();
        }
        let result = op1.wrapping_mul(op2) as u32;
        self.cpsr.set_N((result as i32) < 0);
        self.cpsr.set_Z(result == 0);
        self.gpr[insn.rd()] = result;
        self.S_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_alu_ops(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let rd = insn.rd();

        let (arm_alu_op, shft) = insn.alu_opcode();
        let op1 = if arm_alu_op == AluOpCode::RSB {
            self.get_reg(insn.rs()) as i32
        } else {
            self.get_reg(rd) as i32
        };
        let op2 = if let Some(shft) = shft {
            self.get_barrel_shifted_value(shft)
        } else {
            self.get_reg(insn.rs()) as i32
        };

        let result = self.alu_flags(arm_alu_op, op1, op2);
        if let Some(result) = result {
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
        if OpFormat5::BX == insn.format5_op() {
            self.exec_thumb_bx(sb, insn)
        } else {
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
            let arm_alu_op: AluOpCode = insn.format5_op().into();
            let set_flags = arm_alu_op.is_setting_flags();
            let op1 = self.get_reg(dst_reg) as i32;
            let op2 = self.get_reg(src_reg) as i32;
            let alu_res = if set_flags {
                self.alu_flags(arm_alu_op, op1, op2)
            } else {
                Some(self.alu(arm_alu_op, op1, op2))
            };
            if let Some(result) = alu_res {
                self.set_reg(dst_reg, result as u32);
                if dst_reg == REG_PC {
                    self.flush_pipeline(sb);
                }
            }
            self.S_cycle16(sb, self.pc + 2);
            Ok(())
        }
    }

    fn exec_thumb_ldr_pc(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (insn.pc & !0b10) + 4 + (insn.word8() as Addr);
        self.S_cycle16(sb, self.pc + 2);
        let data = sb.read_32(addr);
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
                sb.write_8(addr, value as u8);
            } else {
                self.N_cycle32(sb, addr);
                sb.write_32(addr, value);
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
                sb.write_16(addr, self.gpr[rd] as u16);
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
        let offset = if insn.is_transferring_bytes() {
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
            sb.write_16(addr, self.gpr[insn.rd()] as u16);
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
            let data = sb.read_32(addr);
            self.S_cycle16(sb, addr);
            self.add_cycle();
            self.gpr[insn.rd()] = data;
        } else {
            sb.write_32(addr, self.gpr[insn.rd()]);
            self.N_cycle16(sb, addr);
        }
        self.N_cycle16(sb, self.pc + 2);
        Ok(())
    }

    fn exec_thumb_add_sp(&mut self, sb: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();
        let arm_alu_op = AluOpCode::ADD;

        self.gpr[REG_SP] = self.alu(arm_alu_op, op1, op2) as u32;
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

        let mut addr = self.gpr[rb];
        let rlist = insn.register_list();
        self.N_cycle16(sb, self.pc);
        let mut first = true;
        if is_load {
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
        } else {
            for r in 0..8 {
                if rlist.bit(r) {
                    if first {
                        first = false;
                    } else {
                        self.S_cycle16(sb, addr);
                    }
                    sb.write_32(addr, self.gpr[r]);
                    addr += 4;
                }
            }
        }

        self.gpr[rb] = addr as u32;

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
            ThumbFormat::Mul => self.exec_thumb_mul(bus, insn),
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
            ThumbFormat::Swi => self.exec_swi(bus),
            ThumbFormat::Branch => self.exec_thumb_branch(bus, insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(bus, insn),
        }
    }
}
