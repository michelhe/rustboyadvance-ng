use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type SendSync<T> = Arc<Mutex<T>>;

use arm7tdmi::gdbstub::common::Signal;
use arm7tdmi::gdbstub::stub::{DisconnectReason, SingleThreadStopReason};
use arm7tdmi::gdbstub::target::TargetError;
use arm7tdmi::gdbstub::target::{ext::base::singlethread::SingleThreadBase, Target};
use arm7tdmi::gdbstub_arch::arm::reg::ArmCoreRegs;
use arm7tdmi::memory::Addr;
use crossbeam::channel::{Receiver, Sender};

// mod target;
mod event_loop;
pub(crate) mod gdb_thread;
mod memory_map;
mod target;

use crate::GameBoyAdvance;

#[derive(Debug)]
pub(crate) enum DebuggerRequest {
    ReadRegs(SendSync<ArmCoreRegs>),
    WriteRegs(ArmCoreRegs),
    ReadAddrs(Addr, SendSync<Box<[u8]>>),
    #[allow(unused)]
    WriteAddrs(Addr, Box<[u8]>),
    AddSwBreakpoint(Addr),
    DelSwBreakpoint(Addr),
    Interrupt,
    Resume,
    SingleStep,
    Disconnected(DisconnectReason),
}

pub struct DebuggerTarget {
    tx: Sender<DebuggerRequest>,
    request_complete_signal: Arc<(Mutex<bool>, Condvar)>,
    stop_signal: Arc<(Mutex<SingleThreadStopReason<u32>>, Condvar)>,
    memory_map: String,
}

impl DebuggerTarget {
    #[inline]
    pub fn wait_for_operation(&mut self) {
        let (lock, cvar) = &*self.request_complete_signal;
        let mut finished = lock.lock().unwrap();
        while !*finished {
            finished = cvar.wait(finished).unwrap();
        }
        *finished = false;
    }
}

pub(crate) struct DebuggerRequestHandler {
    rx: Receiver<DebuggerRequest>,
    request_complete_signal: Arc<(Mutex<bool>, Condvar)>,
    stop_signal: Arc<(Mutex<SingleThreadStopReason<u32>>, Condvar)>,
    thread: JoinHandle<()>,
    stopped: bool,
}

enum DebuggerStatus {
    RequestComplete,
    ResumeRequested,
    StopRequested,
    Disconnected(DisconnectReason),
}

impl DebuggerRequestHandler {
    fn handle_request(
        &mut self,
        gba: &mut GameBoyAdvance,
        req: &mut DebuggerRequest,
    ) -> Result<DebuggerStatus, TargetError<<DebuggerTarget as Target>::Error>> {
        use DebuggerRequest::*;
        match req {
            ReadRegs(regs) => {
                let mut regs = regs.lock().unwrap();
                gba.cpu.read_registers(&mut regs)?;
                debug!("Debugger requested to read regs: {:?}", regs);
                Ok(DebuggerStatus::RequestComplete)
            }
            WriteRegs(regs) => {
                debug!("Debugger requested to write regs: {:?}", regs);
                gba.cpu.write_registers(regs)?;
                Ok(DebuggerStatus::RequestComplete)
            }
            ReadAddrs(addr, data) => {
                let mut data = data.lock().unwrap();
                debug!(
                    "Debugger requested to read {} bytes from 0x{:08x}",
                    data.len(),
                    addr
                );
                gba.cpu.read_addrs(*addr, &mut data)?;
                Ok(DebuggerStatus::RequestComplete)
            }
            WriteAddrs(addr, data) => {
                debug!(
                    "Debugger requested to write {} bytes at 0x{:08x}",
                    data.len(),
                    addr
                );
                gba.cpu.write_addrs(*addr, &data)?;
                Ok(DebuggerStatus::RequestComplete)
            }
            Interrupt => {
                debug!("Debugger requested stopped");
                self.notify_stop_reason(SingleThreadStopReason::Signal(Signal::SIGINT));
                Ok(DebuggerStatus::StopRequested)
            }
            Resume => {
                debug!("Debugger requested resume");
                self.stopped = false;
                Ok(DebuggerStatus::ResumeRequested)
            }
            SingleStep => {
                debug!("Debugger requested single step");
                gba.cpu_step();
                let stop_reason = SingleThreadStopReason::DoneStep;
                self.notify_stop_reason(stop_reason);
                Ok(DebuggerStatus::StopRequested)
            }
            AddSwBreakpoint(addr) => {
                gba.cpu.add_breakpoint(*addr);
                Ok(DebuggerStatus::RequestComplete)
            }
            DelSwBreakpoint(addr) => {
                gba.cpu.del_breakpoint(*addr);
                Ok(DebuggerStatus::RequestComplete)
            }
            Disconnected(reason) => Ok(DebuggerStatus::Disconnected(*reason)),
        }
    }

    fn terminate(mut self, should_interrupt_frame: &mut bool) -> Option<DebuggerRequestHandler> {
        self.notify_stop_reason(SingleThreadStopReason::Exited(1));
        self.thread.join().unwrap();
        self.stopped = true;
        *should_interrupt_frame = true;
        None
    }

    pub fn handle_incoming_requests(
        mut self,
        gba: &mut GameBoyAdvance,
        should_interrupt_frame: &mut bool,
    ) -> Option<DebuggerRequestHandler> {
        if self.thread.is_finished() {
            warn!("gdb server thread unexpectdly died");
            return self.terminate(should_interrupt_frame);
        }
        while let Ok(mut req) = self.rx.try_recv() {
            match self.handle_request(gba, &mut req) {
                Ok(DebuggerStatus::RequestComplete) => {
                    self.notify_request_complete();
                }
                Ok(DebuggerStatus::StopRequested) => {
                    self.stopped = true;
                    self.notify_request_complete();
                }
                Ok(DebuggerStatus::ResumeRequested) => {
                    self.stopped = false;
                    self.notify_request_complete();
                }
                Ok(DebuggerStatus::Disconnected(reason)) => {
                    debug!("Debugger disconnected due to {:?}", reason);
                    debug!("closing gdbserver thread");
                    return self.terminate(should_interrupt_frame);
                }

                Err(_) => {
                    error!("An error occured while handling debug request {:?}", req);
                    return self.terminate(should_interrupt_frame);
                }
            }
        }
        *should_interrupt_frame = self.stopped;
        Some(self)
    }

    fn notify_request_complete(&mut self) {
        let (lock, cvar) = &*self.request_complete_signal;
        let mut finished = lock.lock().unwrap();
        *finished = true;
        cvar.notify_one();
    }

    pub fn notify_stop_reason(&mut self, reason: SingleThreadStopReason<u32>) {
        self.stopped = true;
        let (lock, cvar) = &*self.stop_signal;
        let mut stop_reason = lock.lock().unwrap();
        *stop_reason = reason;
        cvar.notify_one();
    }

    pub fn notify_breakpoint(&mut self, _bp: Addr) {
        self.notify_stop_reason(SingleThreadStopReason::SwBreak(()));
    }
}
