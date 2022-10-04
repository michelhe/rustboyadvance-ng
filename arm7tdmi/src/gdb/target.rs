/// Implementing the Target trait for gdbstub
use gdbstub::common::Signal;
use gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadSingleStep,
};
use gdbstub::target::ext::base::singlethread::{SingleThreadResumeOps, SingleThreadSingleStepOps};
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::ext::breakpoints::BreakpointsOps;
use gdbstub::target::{self, Target, TargetError, TargetResult};

use crate::memory::{DebugRead, MemoryInterface};
use crate::registers_consts::*;
use crate::Arm7tdmiCore;

pub trait MemoryGdbInterface: MemoryInterface + DebugRead {
    fn memory_map_xml(&self, offset: u64, length: usize, buf: &mut [u8]) -> usize;
}

impl<I: MemoryGdbInterface> Target for Arm7tdmiCore<I> {
    type Error = ();
    type Arch = gdbstub_arch::arm::Armv4t;

    #[inline(always)]
    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        BaseOps::SingleThread(self)
    }

    // opt-in to support for setting/removing breakpoints
    #[inline(always)]
    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<Self>> {
        Some(self)
    }

    fn support_memory_map(&mut self) -> Option<target::ext::memory_map::MemoryMapOps<Self>> {
        Some(self)
    }
}

impl<I: MemoryGdbInterface> SingleThreadBase for Arm7tdmiCore<I> {
    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::arm::reg::ArmCoreRegs,
    ) -> TargetResult<(), Self> {
        regs.pc = self.get_next_pc();
        regs.lr = self.get_reg(REG_LR);
        regs.sp = self.get_reg(REG_SP);
        regs.r[..].copy_from_slice(&self.gpr[..13]);
        regs.cpsr = self.cpsr.get();
        Ok(())
    }

    fn write_registers(
        &mut self,
        regs: &gdbstub_arch::arm::reg::ArmCoreRegs,
    ) -> TargetResult<(), Self> {
        self.set_reg(REG_PC, regs.pc);
        self.set_reg(REG_LR, regs.lr);
        self.set_reg(REG_SP, regs.sp);
        self.gpr[..13].copy_from_slice(&regs.r);
        self.cpsr.set(regs.cpsr);
        Ok(())
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<(), Self> {
        self.bus.debug_get_into_bytes(start_addr, data);
        Ok(())
    }

    fn write_addrs(&mut self, _start_addr: u32, _data: &[u8]) -> TargetResult<(), Self> {
        // todo!("implement DebugWrite bus extention")
        Err(TargetError::NonFatal)
    }

    // most targets will want to support at resumption as well...

    #[inline(always)]
    fn support_resume(&mut self) -> Option<SingleThreadResumeOps<Self>> {
        Some(self)
    }
}

impl<I: MemoryGdbInterface> SingleThreadResume for Arm7tdmiCore<I> {
    fn resume(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        // do nothing
        Ok(())
    }

    // ...and if the target supports resumption, it'll likely want to support
    // single-step resume as well

    #[inline(always)]
    fn support_single_step(&mut self) -> Option<SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }
}

impl<I: MemoryGdbInterface> SingleThreadSingleStep for Arm7tdmiCore<I> {
    fn step(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.step();
        Ok(())
    }
}

impl<I: MemoryGdbInterface> target::ext::memory_map::MemoryMap for Arm7tdmiCore<I> {
    fn memory_map_xml(
        &self,
        offset: u64,
        length: usize,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        Ok(self.bus.memory_map_xml(offset, length, buf))
    }
}
