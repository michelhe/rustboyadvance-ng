use std::sync::{Arc, Mutex};

/// Implementing the Target trait for gdbstub
use arm7tdmi::gdb::{copy_range_to_buf, gdbstub, gdbstub_arch};
use gdbstub::common::Signal;
use gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadSingleStep,
};
use gdbstub::target::ext::base::singlethread::{SingleThreadResumeOps, SingleThreadSingleStepOps};
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::ext::breakpoints::BreakpointsOps;
use gdbstub::target::{self, Target, TargetError, TargetResult};
use gdbstub_arch::arm::reg::ArmCoreRegs;

use super::{DebuggerRequest, DebuggerTarget};

impl Target for DebuggerTarget {
    type Error = ();
    type Arch = gdbstub_arch::arm::Armv4t; // as an example

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

impl SingleThreadBase for DebuggerTarget {
    fn read_registers(&mut self, regs: &mut ArmCoreRegs) -> TargetResult<(), Self> {
        let regs_copy = Arc::new(Mutex::new(ArmCoreRegs::default()));
        self.tx
            .send(DebuggerRequest::ReadRegs(regs_copy.clone()))
            .unwrap();
        self.wait_for_operation();
        regs_copy.lock().unwrap().clone_into(regs);
        Ok(())
    }

    fn write_registers(&mut self, regs: &ArmCoreRegs) -> TargetResult<(), Self> {
        self.tx
            .send(DebuggerRequest::WriteRegs(regs.clone()))
            .unwrap();
        self.wait_for_operation();
        Ok(())
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<(), Self> {
        let buffer = Arc::new(Mutex::new(vec![0; data.len()].into_boxed_slice()));
        self.tx
            .send(DebuggerRequest::ReadAddrs(start_addr, buffer.clone()))
            .unwrap();
        self.wait_for_operation();
        data.copy_from_slice(&buffer.lock().unwrap());
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

impl SingleThreadResume for DebuggerTarget {
    fn resume(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.tx.send(DebuggerRequest::Resume).unwrap();
        self.wait_for_operation();
        Ok(())
    }

    // ...and if the target supports resumption, it'll likely want to support
    // single-step resume as well

    #[inline(always)]
    fn support_single_step(&mut self) -> Option<SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }
}

impl SingleThreadSingleStep for DebuggerTarget {
    fn step(&mut self, _signal: Option<Signal>) -> Result<(), Self::Error> {
        self.tx.send(DebuggerRequest::SingleStep).unwrap();
        self.wait_for_operation();
        Ok(())
    }
}

impl target::ext::memory_map::MemoryMap for DebuggerTarget {
    fn memory_map_xml(
        &self,
        offset: u64,
        length: usize,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        Ok(copy_range_to_buf(
            self.memory_map.as_bytes(),
            offset,
            length,
            buf,
        ))
    }
}

impl target::ext::breakpoints::Breakpoints for DebuggerTarget {
    // there are several kinds of breakpoints - this target uses software breakpoints
    #[inline(always)]
    fn support_sw_breakpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
        Some(self)
    }
}

impl target::ext::breakpoints::SwBreakpoint for DebuggerTarget {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.tx
            .send(DebuggerRequest::AddSwBreakpoint(addr))
            .unwrap();
        self.wait_for_operation();
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.tx
            .send(DebuggerRequest::DelSwBreakpoint(addr))
            .unwrap();
        self.wait_for_operation();
        Ok(true)
    }
}
