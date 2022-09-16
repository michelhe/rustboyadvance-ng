use gdbstub::target;
use gdbstub::target::TargetResult;

use crate::Arm7tdmiCore;

use super::target::MemoryGdbInterface;

impl<I: MemoryGdbInterface> target::ext::breakpoints::Breakpoints for Arm7tdmiCore<I> {
    // there are several kinds of breakpoints - this target uses software breakpoints
    #[inline(always)]
    fn support_sw_breakpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
        Some(self)
    }
}

impl<I: MemoryGdbInterface> target::ext::breakpoints::SwBreakpoint for Arm7tdmiCore<I> {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.add_breakpoint(addr);
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.del_breakpoint(addr);
        Ok(true)
    }
}
