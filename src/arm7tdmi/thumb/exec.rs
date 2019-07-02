use crate::arm7tdmi::arm::exec::*;
use crate::arm7tdmi::arm::ArmOpCode;
use crate::arm7tdmi::bus::{Bus, MemoryAccessType::*, MemoryAccessWidth::*};
use crate::arm7tdmi::cpu::{Core, CpuExecResult, CpuPipelineAction};

use super::{ThumbFormat, ThumbInstruction};

impl Core {
    fn exec_thumb_data_process_imm(
        &mut self,
        sysbus: &mut Bus,
        insn: ThumbInstruction,
    ) -> CpuExecResult {
        let arm_alu_op: ArmOpCode = insn.format3_op().into();
        let op1 = self.get_reg(insn.rd()) as i32;
        let op2 = insn.offset8() as i32;
        let result = self.alu(arm_alu_op, op1, op2, true);
        if let Some(result) = result {
            self.set_reg(insn.rd(), result as u32);
        }
        // +1S
        self.add_cycles(
            self.pc + (self.word_size() as u32),
            sysbus,
            Seq + MemoryAccess32,
        );
        Ok(CpuPipelineAction::IncPC)
    }

    pub fn exec_thumb(&mut self, sysbus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        match insn.fmt {
            ThumbFormat::DataProcessImm => self.exec_thumb_data_process_imm(sysbus, insn),
            _ => unimplemented!("thumb not implemented {:#}", insn),
        }
    }
}
