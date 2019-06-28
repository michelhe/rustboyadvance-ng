use crate::bit::BitIndex;

use super::super::{
    cpu::{Core, CpuExecResult, CpuPipelineAction},
    exception::Exception,
    sysbus::SysBus,
    CpuError, CpuInstruction, CpuResult, CpuState, REG_PC,
};

use super::{
    ArmCond, ArmInstruction, ArmInstructionFormat, ArmOpCode, ArmRegisterShift, ArmShiftType,
    ArmShiftedValue,
};

impl Core {
    pub fn exec_arm(&mut self, sysbus: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        if !self.check_arm_cond(insn.cond) {
            return Ok(CpuPipelineAction::IncPC);
        }
        match insn.fmt {
            ArmInstructionFormat::BX => self.exec_bx(sysbus, insn),
            ArmInstructionFormat::B_BL => self.exec_b_bl(sysbus, insn),
            ArmInstructionFormat::DP => self.exec_data_processing(sysbus, insn),
            ArmInstructionFormat::SWI => self.exec_swi(sysbus, insn),
            ArmInstructionFormat::LDR_STR => self.exec_ldr_str(sysbus, insn),
            _ => Err(CpuError::UnimplementedCpuInstruction(CpuInstruction::Arm(
                insn,
            ))),
        }
    }

    fn exec_b_bl(
        &mut self,
        _sysbus: &mut SysBus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        if self.verbose && insn.cond != ArmCond::Always {
        }
        if insn.link_flag() {
            self.set_reg(14, self.pc & !0b1);
        }
        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32;
        Ok(CpuPipelineAction::Flush)
    }

    fn exec_bx(
        &mut self,
        _sysbus: &mut SysBus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        let rn = self.get_reg(insn.rn());
        if rn.bit(0) {
            self.cpsr.set_state(CpuState::THUMB);
        } else {
            self.cpsr.set_state(CpuState::ARM);
        }

        self.pc = rn & !1;
        Ok(CpuPipelineAction::Flush)
    }

    fn exec_swi(
        &mut self,
        _sysbus: &mut SysBus,
        _insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        self.exception(Exception::SoftwareInterrupt);
        Ok(CpuPipelineAction::Flush)
    }

    fn barrel_shift(val: i32, amount: u32, shift: ArmShiftType) -> i32 {
        match shift {
            ArmShiftType::LSL => val.wrapping_shl(amount),
            ArmShiftType::LSR => (val as u32).wrapping_shr(amount) as i32,
            ArmShiftType::ASR => val.wrapping_shr(amount),
            ArmShiftType::ROR => val.rotate_right(amount),
        }
    }

    fn register_shift(&mut self, reg: usize, shift: ArmRegisterShift) -> CpuResult<i32> {
        let val = self.get_reg(reg) as i32;
        match shift {
            ArmRegisterShift::ShiftAmount(amount, shift) => {
                Ok(Core::barrel_shift(val, amount, shift))
            }
            ArmRegisterShift::ShiftRegister(reg, shift) => {
                if reg != REG_PC {
                    Ok(Core::barrel_shift(val, self.get_reg(reg) & 0xff, shift))
                } else {
                    Err(CpuError::IllegalInstruction)
                }
            }
        }
    }

    fn alu_sub_update_carry(a: i32, b: i32, carry: &mut bool) -> i32 {
        let res = a.wrapping_sub(b);
        *carry = res > a;
        res
    }

    fn alu_add_update_carry(a: i32, b: i32, carry: &mut bool) -> i32 {
        let res = a.wrapping_sub(b);
        *carry = res < a;
        res
    }

    fn alu(&mut self, opcode: ArmOpCode, op1: i32, op2: i32, set_cond_flags: bool) -> Option<i32> {
        let C = self.cpsr.C() as i32;

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();

        let result = match opcode {
            ArmOpCode::AND | ArmOpCode::TST => op1 & op2,
            ArmOpCode::EOR | ArmOpCode::TEQ => op1 ^ op2,
            ArmOpCode::SUB | ArmOpCode::CMP => Self::alu_sub_update_carry(op1, op2, &mut carry),
            ArmOpCode::RSB => Self::alu_sub_update_carry(op2, op1, &mut carry),
            ArmOpCode::ADD | ArmOpCode::CMN => Self::alu_add_update_carry(op1, op2, &mut carry),
            ArmOpCode::ADC => Self::alu_add_update_carry(op1, op2.wrapping_add(C), &mut carry),
            ArmOpCode::SBC => Self::alu_add_update_carry(op1, op2.wrapping_sub(1 - C), &mut carry),
            ArmOpCode::RSC => Self::alu_add_update_carry(op2, op1.wrapping_sub(1 - C), &mut carry),
            ArmOpCode::ORR => op1 | op2,
            ArmOpCode::MOV => op2,
            ArmOpCode::BIC => op1 & (!op2),
            ArmOpCode::MVN => !op2,
        };

        if set_cond_flags {
            self.cpsr.set_N(result < 0);
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(carry);
            self.cpsr.set_V(overflow);
        }

        match opcode {
            ArmOpCode::TST | ArmOpCode::TEQ | ArmOpCode::CMP | ArmOpCode::CMN => None,
            _ => Some(result),
        }
    }

    fn exec_data_processing(
        &mut self,
        _sysbus: &mut SysBus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        // TODO handle carry flag

        let op1 = self.get_reg(insn.rn()) as i32;
        let op2 = insn.operand2()?;

        let op2: i32 = match op2 {
            ArmShiftedValue::RotatedImmediate(immediate, rotate) => {
                Ok(immediate.rotate_right(rotate) as i32)
            }
            ArmShiftedValue::ShiftedRegister {
                reg,
                shift,
                added: _,
            } => self.register_shift(reg, shift),
            _ => unreachable!(),
        }?;

        let opcode = insn.opcode().unwrap();
        let set_flags = opcode.is_setting_flags() || insn.set_cond_flag();
        if let Some(result) = self.alu(opcode, op1, op2, set_flags) {
            self.set_reg(insn.rd(), result as u32)
        }

        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_ldr_str(
        &mut self,
        _sysbus: &mut SysBus,
        insn: ArmInstruction,
    ) -> CpuResult<CpuPipelineAction> {
        let rn = self.get_reg(insn.rn());
        let rd = self.get_reg(insn.rd());
        Ok(CpuPipelineAction::IncPC)
    }
}
