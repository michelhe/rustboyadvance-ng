use super::super::super::sysbus::SysBus;
use super::super::cpu::{Core, CpuExecResult};
use super::ThumbInstruction;

impl Core {
    pub fn exec_thumb(&mut self, sysbus: &mut SysBus, insn: ThumbInstruction) -> CpuExecResult {
        unimplemented!("thumb not implemented {:#}", insn)
    }
}
