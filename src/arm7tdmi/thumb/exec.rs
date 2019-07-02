use super::super::cpu::{Core, CpuExecResult};
use super::ThumbInstruction;
use crate::arm7tdmi::bus::Bus;

impl Core {
    pub fn exec_thumb(&mut self, sysbus: &mut Bus, insn: ThumbInstruction) -> CpuExecResult {
        unimplemented!("thumb not implemented {:#}", insn)
    }
}
