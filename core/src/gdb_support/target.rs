use arm7tdmi::gdb::target::MemoryGdbInterface;
/// Implementing the Target trait for gdbstub
use arm7tdmi::gdb::{gdbstub as _gdbstub, gdbstub_arch as _gdbstub_arch};

use _gdbstub::common::Signal;
use _gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadSingleStep,
};
use _gdbstub::target::ext::base::singlethread::{SingleThreadResumeOps, SingleThreadSingleStepOps};
use _gdbstub::target::ext::base::BaseOps;
use _gdbstub::target::ext::breakpoints::BreakpointsOps;
use _gdbstub::target::{self, Target, TargetError, TargetResult};

use crate::GameBoyAdvance;

impl Target for GameBoyAdvance {
    type Error = ();
    type Arch = _gdbstub_arch::arm::Armv4t; // as an example

    #[inline(always)]
    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        self.cpu.base_ops()
    }

    // opt-in to support for setting/removing breakpoints
    #[inline(always)]
    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<Self>> {
        self.cpu.support_breakpoints()
    }

    fn support_memory_map(&mut self) -> Option<target::ext::memory_map::MemoryMapOps<Self>> {
        self.cpu.support_memory_map()
    }
}

impl SingleThreadBase for GameBoyAdvance {
    fn read_registers(
        &mut self,
        regs: &mut _gdbstub_arch::arm::reg::ArmCoreRegs,
    ) -> TargetResult<(), Self> {
        self.cpu.read_registers(regs)
    }

    fn write_registers(
        &mut self,
        regs: &_gdbstub_arch::arm::reg::ArmCoreRegs,
    ) -> TargetResult<(), Self> {
        self.cpu.write_registers(regs)
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<(), Self> {
        self.cpu.read_addrs(start_addr, data)
    }

    fn write_addrs(&mut self, _start_addr: u32, _data: &[u8]) -> TargetResult<(), Self> {
        // todo!("implement DebugWrite bus extention")
        Err(TargetError::NonFatal)
    }

    // most targets will want to support at resumption as well...

    #[inline(always)]
    fn support_resume(&mut self) -> Option<SingleThreadResumeOps<Self>> {
        self.cpu.support_resume()
    }
}

impl SingleThreadResume for GameBoyAdvance {
    fn resume(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        self.cpu.resume(signal)
    }

    // ...and if the target supports resumption, it'll likely want to support
    // single-step resume as well

    #[inline(always)]
    fn support_single_step(&mut self) -> Option<SingleThreadSingleStepOps<'_, Self>> {
        self.cpu.support_single_step()
    }
}

impl SingleThreadSingleStep for GameBoyAdvance {
    fn step(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.key_poll();
        // Running for 1 cycle to get only one cpu step
        self.run::<true>(1);
        Ok(())
    }
}

impl target::ext::memory_map::MemoryMap for GameBoyAdvance {
    fn memory_map_xml(
        &self,
        offset: u64,
        length: usize,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        Ok(self.sysbus.memory_map_xml(offset, length, buf))
    }
}

impl target::ext::breakpoints::Breakpoints for GameBoyAdvance {
    // there are several kinds of breakpoints - this target uses software breakpoints
    #[inline(always)]
    fn support_sw_breakpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
        self.cpu.support_sw_breakpoint()
    }
}

impl target::ext::breakpoints::SwBreakpoint for GameBoyAdvance {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        kind: _gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.cpu.add_sw_breakpoint(addr, kind)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        kind: _gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.cpu.remove_sw_breakpoint(addr, kind)
    }
}
