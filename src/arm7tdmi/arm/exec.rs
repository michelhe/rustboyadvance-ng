use super::super::cpu::{Core, CpuPipelineAction, CpuError, CpuInstruction, CpuExecResult};
use super::super::sysbus::SysBus;
use super::{ArmInstruction, ArmInstructionFormat};

impl Core {
    pub fn exec_arm(&mut self, sysbus: &mut SysBus, insn: ArmInstruction) -> CpuExecResult {
        match insn.fmt {
            ArmInstructionFormat::BX => {
                self.pc = self.get_reg(insn.rn());
                Ok((CpuInstruction::Arm(insn), CpuPipelineAction::Branch))
            },
            ArmInstructionFormat::B_BL => {
                self.pc = (self.pc as i32).wrapping_add(insn.branch_offset()) as u32;
                Ok((CpuInstruction::Arm(insn), CpuPipelineAction::Branch))
            }
            fmt => Err(CpuError::UnimplementedCpuInstruction(CpuInstruction::Arm(insn))),
        }
    }
}