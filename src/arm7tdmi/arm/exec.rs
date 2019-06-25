use super::super::cpu::{Core, CpuState, CpuPipelineAction, CpuError, CpuInstruction, CpuResult, CpuExecResult};
use super::super::sysbus::SysBus;
use super::{ArmInstruction, ArmInstructionFormat};

use crate::bit::BitIndex;

impl Core {
    fn exec_b_bl(&mut self, sysbus: &mut SysBus, insn: ArmInstruction) -> CpuResult<CpuPipelineAction> {
        if insn.link_flag() {
            self.set_reg(14, self.pc & !0b1);
        }
        self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32;
        Ok(CpuPipelineAction::Branch)
    }

    fn exec_bx(&mut self, sysbus: &mut SysBus, insn: ArmInstruction) -> CpuResult<CpuPipelineAction> {
        let rn = self.get_reg(insn.rn());
        if rn.bit(0) {
            self.set_state(CpuState::THUMB);
        } else {
            self.set_state(CpuState::ARM);
        }

        Ok(CpuPipelineAction::Branch)
    }

    pub fn exec_arm(&mut self, sysbus: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        let action = match insn.fmt {
            ArmInstructionFormat::BX => self.exec_bx(sysbus, insn),
            ArmInstructionFormat::B_BL => self.exec_b_bl(sysbus, insn),
            fmt => Err(CpuError::UnimplementedCpuInstruction(CpuInstruction::Arm(insn))),
        }?;
        Ok((CpuInstruction::Arm(insn), action))
    }
}