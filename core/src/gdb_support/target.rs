use std::sync::{Arc, Condvar, Mutex, WaitTimeoutResult};
use std::time::Duration;

/// Implementing the Target trait for gdbstub
use arm7tdmi::gdb::{copy_range_to_buf, gdbstub, gdbstub_arch};
use crossbeam::channel::Sender;
use gdbstub::common::Signal;
use gdbstub::stub::{DisconnectReason, SingleThreadStopReason};
use gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadSingleStep,
};
use gdbstub::target::ext::base::singlethread::{SingleThreadResumeOps, SingleThreadSingleStepOps};
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::ext::breakpoints::BreakpointsOps;
use gdbstub::target::ext::monitor_cmd::{outputln, ConsoleOutput};
use gdbstub::target::{self, Target, TargetError, TargetResult};
use gdbstub_arch::arm::reg::ArmCoreRegs;

use super::DebuggerRequest;

pub(crate) struct DebuggerTarget {
    tx: Sender<DebuggerRequest>,
    request_complete_signal: Arc<(Mutex<bool>, Condvar)>,
    pub(crate) stop_signal: Arc<(Mutex<Option<SingleThreadStopReason<u32>>>, Condvar)>,
    pub(crate) memory_map: String,
}

impl DebuggerTarget {
    pub fn new(
        tx: Sender<DebuggerRequest>,
        request_complete_signal: Arc<(Mutex<bool>, Condvar)>,
        stop_signal: Arc<(Mutex<Option<SingleThreadStopReason<u32>>>, Condvar)>,
        memory_map: String,
    ) -> DebuggerTarget {
        DebuggerTarget {
            tx,
            request_complete_signal,
            stop_signal,
            memory_map,
        }
    }

    pub fn debugger_request(&mut self, req: DebuggerRequest) {
        let (lock, cvar) = &*self.request_complete_signal;
        let mut finished = lock.lock().unwrap();
        // now send the request
        self.tx.send(req).unwrap();
        // wait for the notification
        while !*finished {
            finished = cvar.wait(finished).unwrap();
        }
        // ack the other side we got the signal
        *finished = false;
        cvar.notify_one();
    }

    pub fn wait_for_stop_reason_timeout(
        &mut self,
        timeout: Duration,
    ) -> Option<SingleThreadStopReason<u32>> {
        let (lock, cvar) = &*self.stop_signal;
        let mut stop_reason = lock.lock().unwrap();
        let mut timeout_result: WaitTimeoutResult;
        while stop_reason.is_none() {
            (stop_reason, timeout_result) = cvar.wait_timeout(stop_reason, timeout).unwrap();
            if timeout_result.timed_out() {
                return None;
            }
        }
        Some(stop_reason.take().expect("None is not expected here"))
    }

    pub fn wait_for_stop_reason_blocking(&mut self) -> SingleThreadStopReason<u32> {
        let (lock, cvar) = &*self.stop_signal;
        let mut stop_reason = lock.lock().unwrap();
        while stop_reason.is_none() {
            stop_reason = cvar.wait(stop_reason).unwrap();
        }
        stop_reason.take().expect("None is not expected here")
    }

    pub fn disconnect(&mut self, disconnect_reason: DisconnectReason) {
        self.tx
            .send(DebuggerRequest::Disconnected(disconnect_reason))
            .unwrap();
    }
}

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

    fn support_monitor_cmd(&mut self) -> Option<target::ext::monitor_cmd::MonitorCmdOps<'_, Self>> {
        Some(self)
    }
}

impl SingleThreadBase for DebuggerTarget {
    fn read_registers(&mut self, regs: &mut ArmCoreRegs) -> TargetResult<(), Self> {
        let regs_copy = Arc::new(Mutex::new(ArmCoreRegs::default()));
        self.debugger_request(DebuggerRequest::ReadRegs(regs_copy.clone()));
        regs_copy.lock().unwrap().clone_into(regs);
        Ok(())
    }

    fn write_registers(&mut self, regs: &ArmCoreRegs) -> TargetResult<(), Self> {
        self.debugger_request(DebuggerRequest::WriteRegs(regs.clone()));
        Ok(())
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<(), Self> {
        let buffer = Arc::new(Mutex::new(vec![0; data.len()].into_boxed_slice()));
        self.debugger_request(DebuggerRequest::ReadAddrs(start_addr, buffer.clone()));
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
        self.debugger_request(DebuggerRequest::Resume);
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
        self.debugger_request(DebuggerRequest::SingleStep);
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
        self.debugger_request(DebuggerRequest::AddSwBreakpoint(addr));
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        _kind: gdbstub_arch::arm::ArmBreakpointKind,
    ) -> TargetResult<bool, Self> {
        self.debugger_request(DebuggerRequest::DelSwBreakpoint(addr));
        Ok(true)
    }
}

impl target::ext::monitor_cmd::MonitorCmd for DebuggerTarget {
    fn handle_monitor_cmd(
        &mut self,
        cmd: &[u8],
        mut out: ConsoleOutput<'_>,
    ) -> Result<(), Self::Error> {
        let cmd = match std::str::from_utf8(cmd) {
            Ok(cmd) => cmd,
            Err(_) => {
                outputln!(out, "command must be valid UTF-8");
                return Ok(());
            }
        };

        match cmd {
            "reset" => {
                self.debugger_request(DebuggerRequest::Reset);
                outputln!(out, "sent reset signal");
            }
            unk => {
                outputln!(out, "unknown command: {}", unk);
            }
        }

        Ok(())
    }
}
