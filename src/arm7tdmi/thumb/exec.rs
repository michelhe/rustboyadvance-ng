use crate::arm7tdmi::arm::*;
use crate::arm7tdmi::bus::Bus;
use crate::arm7tdmi::cpu::{Core, CpuExecResult, CpuPipelineAction};
use crate::arm7tdmi::*;

use super::*;
fn push(cpu: &mut Core, bus: &mut Bus, r: usize) {
    cpu.gpr[REG_SP] -= 4;
    let stack_addr = cpu.gpr[REG_SP];
    cpu.store_32(stack_addr, cpu.get_reg(r), bus)
}
fn pop(cpu: &mut Core, bus: &mut Bus, r: usize) {
    let stack_addr = cpu.gpr[REG_SP];
    let val = cpu.load_32(stack_addr, bus);
    cpu.set_reg(r, val);
    cpu.gpr[REG_SP] = stack_addr + 4;
}

impl Core {
    fn exec_thumb_move_shifted_reg(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let result = self
            .register_shift(
                insn.rs(),
                ArmRegisterShift::ShiftAmount(insn.offset5() as u8 as u32, insn.format1_op()),
            )
            .unwrap();
        self.gpr[insn.rd()] = result as u32;

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_add_sub(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.get_reg(insn.rs()) as i32;
        let op2 = if insn.is_immediate_operand() {
            insn.rn() as u32 as i32
        } else {
            self.get_reg(insn.rn()) as i32
        };
        let arm_alu_op = if insn.is_subtract() {
            ArmOpCode::SUB
        } else {
            ArmOpCode::ADD
        };

        let result = self.alu(arm_alu_op, op1, op2, true);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_data_process_imm(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let arm_alu_op: ArmOpCode = insn.format3_op().into();
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = insn.offset8() as u8 as i32;
        let result = self.alu(arm_alu_op, op1, op2, true);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_mul(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = self.get_reg(insn.rs()) as i32;
        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.add_cycle();
        }
        self.gpr[insn.rd()] = (op1 * op2) as u32;
        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_alu_ops(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let arm_alu_op: ArmOpCode = insn.alu_opcode();
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = self.get_reg(insn.rs()) as i32;
        let result = self.alu(arm_alu_op, op1, op2, true);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }

        Ok(CpuPipelineAction::IncPC)
    }

    /// Cycles 2S+1N
    fn exec_thumb_bx(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let src_reg = if insn.flag(ThumbInstruction::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };

        let addr = self.get_reg(src_reg);
        if addr.bit(0) {
            self.cpsr.set_state(CpuState::THUMB);
        } else {
            self.cpsr.set_state(CpuState::ARM);
        }

        self.pc = addr & !1;

        Ok(CpuPipelineAction::Flush)
    }

    fn exec_thumb_hi_reg_op_or_bx(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        if OpFormat5::BX == insn.format5_op() {
            self.exec_thumb_bx(bus, insn)
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
            let arm_alu_op: ArmOpCode = insn.format5_op().into();
            let op1 = self.gpr[dst_reg] as i32;
            let op2 = self.gpr[src_reg] as i32;
            let result = self.alu(arm_alu_op, op1, op2, true);
            if let Some(result) = result {
                self.gpr[dst_reg] = result as u32;
            }
            Ok(CpuPipelineAction::IncPC)
        }
    }

    fn exec_thumb_ldr_pc(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (insn.pc & !0b10) + 4 + (insn.word8() as Addr);
        let data = self.load_32(addr, bus);

        self.set_reg(insn.rd(), data);
        // +1I
        self.add_cycle();

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_ldr_str_reg_offset(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let addr = self
            .get_reg(insn.rb())
            .wrapping_add(self.get_reg(insn.ro()));
        if insn.is_load() {
            let data = if insn.is_transfering_bytes() {
                self.load_8(addr, bus) as u32
            } else {
                self.load_32(addr, bus)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();
        } else {
            let value = self.get_reg(insn.rd());
            if insn.is_transfering_bytes() {
                self.store_8(addr, value as u8, bus);
            } else {
                self.store_32(addr, value, bus);
            };
        }

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_ldr_str_halfword(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let base = self.gpr[insn.rb()] as i32;
        let addr = base.wrapping_add((insn.offset5() << 1) as i32) as Addr;
        if insn.is_load() {
            let data = self.load_16(addr, bus);
            self.add_cycle();
            self.gpr[insn.rd()] = data as u32;
        } else {
            self.store_16(addr, self.gpr[insn.rd()] as u16, bus);
        }
        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_ldr_str_sp(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (self.gpr[REG_SP] & !0b10) + 4 + (insn.word8() as Addr);
        if insn.is_load() {
            let data = self.load_32(addr, bus);
            self.add_cycle();
            self.gpr[insn.rd()] = data;
        } else {
            self.store_32(addr, self.gpr[insn.rd()], bus);
        }
        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_add_sp(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();
        let arm_alu_op = ArmOpCode::ADD;

        let result = self.alu(arm_alu_op, op1, op2, false);
        if let Some(result) = result {
            self.gpr[REG_SP] = result as u32;
        }

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_push_pop(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).

        let is_pop = insn.is_load();
        let mut pipeline_action = CpuPipelineAction::IncPC;

        let pc_lr_flag = insn.flag(ThumbInstruction::FLAG_R);
        let rlist = insn.register_list();
        if is_pop {
            for r in rlist {
                pop(self, bus, r);
            }
            if pc_lr_flag {
                pop(self, bus, REG_PC);
                pipeline_action = CpuPipelineAction::Flush;
            }
            self.add_cycle();
        } else {
            if pc_lr_flag {
                push(self, bus, REG_LR);
            }
            for r in rlist.into_iter().rev() {
                push(self, bus, r);
            }
        }

        Ok(pipeline_action)
    }

    fn exec_thumb_branch_with_cond(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond()) {
            Ok(CpuPipelineAction::IncPC)
        } else {
            let offset = ((insn.offset8() as i8) << 1) as i32;
            self.pc = (self.pc as i32).wrapping_add(offset) as u32;
            Ok(CpuPipelineAction::Flush)
        }
    }

    fn exec_thumb_branch_long_with_link(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let mut off = insn.offset11();
        if insn.flag(ThumbInstruction::FLAG_LOW_OFFSET) {
            off = off << 1;
            let next_pc = (self.pc - 2) | 1;
            self.pc = (self.gpr[REG_LR] as i32).wrapping_add(off) as u32;
            self.gpr[REG_LR] = next_pc;

            Ok(CpuPipelineAction::Flush)
        } else {
            off = (off << 21) >> 9;
            self.gpr[REG_LR] = (self.pc as i32).wrapping_add(off) as u32;

            Ok(CpuPipelineAction::IncPC)
        }
    }

    pub fn exec_thumb(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        match insn.fmt {
            ThumbFormat::MoveShiftedReg => self.exec_thumb_move_shifted_reg(bus, insn),
            ThumbFormat::AddSub => self.exec_thumb_add_sub(bus, insn),
            ThumbFormat::DataProcessImm => self.exec_thumb_data_process_imm(bus, insn),
            ThumbFormat::Mul => self.exec_thumb_mul(bus, insn),
            ThumbFormat::AluOps => self.exec_thumb_alu_ops(bus, insn),
            ThumbFormat::HiRegOpOrBranchExchange => self.exec_thumb_hi_reg_op_or_bx(bus, insn),
            ThumbFormat::LdrPc => self.exec_thumb_ldr_pc(bus, insn),
            ThumbFormat::LdrStrRegOffset => self.exec_thumb_ldr_str_reg_offset(bus, insn),
            ThumbFormat::LdrStrHalfWord => self.exec_thumb_ldr_str_halfword(bus, insn),
            ThumbFormat::LdrStrSp => self.exec_thumb_ldr_str_sp(bus, insn),
            ThumbFormat::AddSp => self.exec_thumb_add_sp(bus, insn),
            ThumbFormat::PushPop => self.exec_thumb_push_pop(bus, insn),
            ThumbFormat::BranchConditional => self.exec_thumb_branch_with_cond(bus, insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(bus, insn),
            _ => unimplemented!("thumb not implemented {:#x?}", insn),
        }
    }
}
