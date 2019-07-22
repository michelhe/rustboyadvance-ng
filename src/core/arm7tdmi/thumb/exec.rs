use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::cpu::{Core, CpuExecResult};
use crate::core::arm7tdmi::*;

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

        Ok(())
    }

    fn exec_thumb_add_sub(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
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

        Ok(())
    }

    fn exec_thumb_data_process_imm(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let arm_alu_op: AluOpCode = insn.format3_op().into();
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = insn.offset8() as u8 as i32;
        let result = self.alu_flags(arm_alu_op, op1, op2);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }

        Ok(())
    }

    fn exec_thumb_mul(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = self.get_reg(insn.rs()) as i32;
        let m = self.get_required_multipiler_array_cycles(op2);
        for _ in 0..m {
            self.add_cycle();
        }
        self.gpr[insn.rd()] = op1.wrapping_mul(op2) as u32;
        Ok(())
    }

    fn exec_thumb_alu_ops(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let rd = insn.rd();

        let (arm_alu_op, shft) = insn.alu_opcode();
        let op1 = self.get_reg(rd) as i32;
        let op2 = if let Some(shft) = shft {
            self.get_barrel_shifted_value(shft)
        } else {
            self.get_reg(insn.rs()) as i32
        };

        let result = self.alu_flags(arm_alu_op, op1, op2);
        if let Some(result) = result {
            self.set_reg(rd, result as u32);
        }

        Ok(())
    }

    /// Cycles 2S+1N
    fn exec_thumb_bx(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let src_reg = if insn.flag(ThumbInstruction::FLAG_H2) {
            insn.rs() + 8
        } else {
            insn.rs()
        };
        self.branch_exchange(self.get_reg(src_reg))
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
                    self.flush_pipeline();
                }
            }
            Ok(())
        }
    }

    fn exec_thumb_ldr_pc(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (insn.pc & !0b10) + 4 + (insn.word8() as Addr);
        let data = self.load_32(addr, bus);

        self.set_reg(insn.rd(), data);
        // +1I
        self.add_cycle();

        Ok(())
    }

    fn do_exec_thumb_ldr_str(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
        addr: Addr,
    ) -> CpuExecResult {
        if insn.is_load() {
            let data = if insn.is_transferring_bytes() {
                self.load_8(addr, bus) as u32
            } else {
                self.ldr_word(addr, bus)
            };

            self.set_reg(insn.rd(), data);

            // +1I
            self.add_cycle();
        } else {
            let value = self.get_reg(insn.rd());
            if insn.is_transferring_bytes() {
                self.store_8(addr, value as u8, bus);
            } else {
                self.store_32(addr, value, bus);
            };
        }

        Ok(())
    }

    fn exec_thumb_ldr_str_reg_offset(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let addr = self
            .get_reg(insn.rb())
            .wrapping_add(self.get_reg(insn.ro()));
        self.do_exec_thumb_ldr_str(bus, insn, addr)
    }

    fn exec_thumb_ldr_str_shb(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
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
                self.store_16(addr, self.gpr[rd] as u16, bus)
            }
            (false, true) =>
            /* ldrh */
            {
                self.gpr[rd] = self.ldr_half(addr, bus)
            }
            (true, false) =>
            /* ldsb */
            {
                let val = self.load_8(addr, bus) as i8 as i32 as u32;
                self.gpr[rd] = val;
            }
            (true, true) =>
            /* ldsh */
            {
                let val = self.ldr_sign_half(addr, bus);
                self.gpr[rd] = val;
            }
        }

        Ok(())
    }

    fn exec_thumb_ldr_str_imm_offset(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let offset = if insn.is_transferring_bytes() {
            insn.offset5()
        } else {
            (insn.offset5() << 3) >> 1
        };
        let addr = self.get_reg(insn.rb()).wrapping_add(offset as u32);
        self.do_exec_thumb_ldr_str(bus, insn, addr)
    }

    fn exec_thumb_ldr_str_halfword(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let base = self.gpr[insn.rb()] as i32;
        let addr = base.wrapping_add((insn.offset5() << 1) as i32) as Addr;
        if insn.is_load() {
            let data = self.ldr_half(addr, bus);
            self.add_cycle();
            self.gpr[insn.rd()] = data as u32;
        } else {
            self.store_16(addr, self.gpr[insn.rd()] as u16, bus);
        }
        Ok(())
    }

    fn exec_thumb_ldr_str_sp(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = self.gpr[REG_SP] + (insn.word8() as Addr);
        self.do_exec_thumb_ldr_str_with_addr(bus, insn, addr)
    }

    fn exec_thumb_load_address(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let result = if insn.flag(ThumbInstruction::FLAG_SP) {
            self.gpr[REG_SP] + (insn.word8() as Addr)
        } else {
            (insn.pc & !0b10) + 4 + (insn.word8() as Addr)
        };
        self.gpr[insn.rd()] = result;

        Ok(())
    }

    fn do_exec_thumb_ldr_str_with_addr(
        &mut self,
        bus: &mut Bus,
        insn: ThumbInstruction,
        addr: Addr,
    ) -> CpuExecResult {
        if insn.is_load() {
            let data = self.load_32(addr, bus);
            self.add_cycle();
            self.gpr[insn.rd()] = data;
        } else {
            self.store_32(addr, self.gpr[insn.rd()], bus);
        }
        Ok(())
    }

    fn exec_thumb_add_sp(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let op1 = self.gpr[REG_SP] as i32;
        let op2 = insn.sword7();
        let arm_alu_op = AluOpCode::ADD;

        self.gpr[REG_SP] = self.alu(arm_alu_op, op1, op2) as u32;

        Ok(())
    }

    fn exec_thumb_push_pop(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).
        let is_pop = insn.is_load();
        let pc_lr_flag = insn.flag(ThumbInstruction::FLAG_R);
        let rlist = insn.register_list();
        if is_pop {
            for r in 0..8 {
                if rlist.bit(r) {
                    pop(self, bus, r);
                }
            }
            if pc_lr_flag {
                pop(self, bus, REG_PC);
                self.pc = self.pc & !1;
                self.flush_pipeline();
            }
            self.add_cycle();
        } else {
            if pc_lr_flag {
                push(self, bus, REG_LR);
            }
            for r in (0..8).rev() {
                if rlist.bit(r) {
                    push(self, bus, r);
                }
            }
        }

        Ok(())
    }

    fn exec_thumb_ldm_stm(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        // (From GBATEK) Execution Time: nS+1N+1I (POP), (n+1)S+2N+1I (POP PC), or (n-1)S+2N (PUSH).

        let is_load = insn.is_load();
        let rb = insn.rb();

        let mut addr = self.gpr[rb];
        let rlist = insn.register_list();
        if is_load {
            for r in 0..8 {
                if rlist.bit(r) {
                    let val = self.load_32(addr, bus);
                    addr += 4;
                    self.add_cycle();
                    self.set_reg(r, val);
                }
            }
        } else {
            for r in 0..8 {
                if rlist.bit(r) {
                    self.store_32(addr, self.gpr[r], bus);
                    addr += 4;
                }
            }
        }

        self.gpr[rb] = addr as u32;

        Ok(())
    }

    fn exec_thumb_branch_with_cond(
        &mut self,
        _bus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond()) {
            Ok(())
        } else {
            let offset = ((insn.offset8() as i8) << 1) as i32;
            self.pc = (self.pc as i32).wrapping_add(offset) as u32;
            self.flush_pipeline();
            Ok(())
        }
    }

    fn exec_thumb_branch(&mut self, _bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let offset = ((insn.offset11() << 21) >> 20) as i32;
        self.pc = (self.pc as i32).wrapping_add(offset) as u32;
        self.flush_pipeline();
        Ok(())
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

            self.flush_pipeline();
            Ok(())
        } else {
            off = (off << 21) >> 9;
            self.gpr[REG_LR] = (self.pc as i32).wrapping_add(off) as u32;

            Ok(())
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
            ThumbFormat::LdrStrSHB => self.exec_thumb_ldr_str_shb(bus, insn),
            ThumbFormat::LdrStrImmOffset => self.exec_thumb_ldr_str_imm_offset(bus, insn),
            ThumbFormat::LdrStrHalfWord => self.exec_thumb_ldr_str_halfword(bus, insn),
            ThumbFormat::LdrStrSp => self.exec_thumb_ldr_str_sp(bus, insn),
            ThumbFormat::LoadAddress => self.exec_thumb_load_address(bus, insn),
            ThumbFormat::AddSp => self.exec_thumb_add_sp(bus, insn),
            ThumbFormat::PushPop => self.exec_thumb_push_pop(bus, insn),
            ThumbFormat::LdmStm => self.exec_thumb_ldm_stm(bus, insn),
            ThumbFormat::BranchConditional => self.exec_thumb_branch_with_cond(bus, insn),
            ThumbFormat::Swi => self.exec_swi(),
            ThumbFormat::Branch => self.exec_thumb_branch(bus, insn),
            ThumbFormat::BranchLongWithLink => self.exec_thumb_branch_long_with_link(bus, insn),
            _ => unimplemented!("thumb not implemented {:#x?}", insn),
        }
    }
}
