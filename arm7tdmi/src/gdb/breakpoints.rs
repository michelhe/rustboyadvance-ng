use gdbstub::target;
use gdbstub::target::TargetResult;
use log::debug;

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
        debug!("adding breakpoint {:08x}", addr);
        self.breakpoints.push(addr);
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        match self.breakpoints.iter().position(|x| *x == addr) {
            None => Ok(false),
            Some(pos) => {
                debug!("deleting breakpoint {:08x}", addr);
                self.breakpoints.remove(pos);
                Ok(true)
            }
        }
    }
}

impl<I: MemoryGdbInterface> Arm7tdmiCore<I> {
    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.get_next_pc();
        for bp in &self.breakpoints {
            if (*bp & !1) == next_pc {
                return Some(*bp);
            }
        }
        None
    }
}
