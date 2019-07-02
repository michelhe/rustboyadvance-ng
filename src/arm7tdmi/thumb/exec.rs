use crate::arm7tdmi::arm::*;
use crate::arm7tdmi::bus::{Bus, MemoryAccessType::*, MemoryAccessWidth::*};
use crate::arm7tdmi::cpu::{Core, CpuExecResult, CpuPipelineAction};
use crate::arm7tdmi::{Addr, REG_PC};

use super::{ThumbFormat, ThumbInstruction};

impl Core {
    fn exec_thumb_data_process_imm(
        &mut self,
        bus: &mut Bus,
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
            bus,
            Seq + MemoryAccess32,
        );
        Ok(CpuPipelineAction::IncPC)
    }

    fn exec_thumb_ldr_pc(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        let addr = (insn.pc & !0b10) + 4 + (insn.word8() as Addr);
        let data = bus.read_32(addr);
        // +1N
        self.add_cycles(addr, bus, NonSeq + MemoryAccess32);
        // +1S
        self.add_cycles(
            self.pc + (self.word_size() as u32),
            bus,
            Seq + MemoryAccess32,
        );
        self.set_reg(insn.rd(), data);
        // +1I
        self.add_cycle();

        Ok(CpuPipelineAction::IncPC)
    }

    pub fn exec_thumb(&mut self, bus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        match insn.fmt {
            ThumbFormat::DataProcessImm => self.exec_thumb_data_process_imm(bus, insn),
            ThumbFormat::LdrPc => self.exec_thumb_ldr_pc(bus, insn),
            _ => unimplemented!("thumb not implemented {:#}", insn),
        }
    }
}
