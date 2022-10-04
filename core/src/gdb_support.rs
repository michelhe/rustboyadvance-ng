use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type SendSync<T> = Arc<Mutex<T>>;

use arm7tdmi::gdbstub::common::Signal;
use arm7tdmi::gdbstub::stub::{DisconnectReason, SingleThreadStopReason};
use arm7tdmi::gdbstub::target::TargetError;
use arm7tdmi::gdbstub::target::{ext::base::singlethread::SingleThreadBase, Target};
use arm7tdmi::gdbstub_arch::arm::reg::ArmCoreRegs;
use arm7tdmi::memory::Addr;
use crossbeam::channel::Receiver;

// mod target;
mod event_loop;
pub(crate) mod gdb_thread;
mod memory_map;
mod target;
use target::DebuggerTarget;

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
    Reset,
    Disconnected(DisconnectReason),
}

pub(crate) struct DebuggerRequestHandler {
    rx: Receiver<DebuggerRequest>,
    request_complete_signal: Arc<(Mutex<bool>, Condvar)>,
    stop_signal: Arc<(Mutex<Option<SingleThreadStopReason<u32>>>, Condvar)>,
    thread: JoinHandle<()>,
    pub(crate) stopped: bool,
}

impl DebuggerRequestHandler {
    pub fn handle_incoming_requests(
        mut self,
        gba: &mut GameBoyAdvance,
    ) -> Option<DebuggerRequestHandler> {
        if self.thread.is_finished() {
            warn!("gdb server thread unexpectdly died");
            return self.terminate();
        }
        loop {
            // Handle as much as messages as possible
            while let Ok(mut req) = self.rx.try_recv() {
                match self.handle_request(gba, &mut req) {
                    Ok(Some(disconnect_reason)) => {
                        debug!("Debugger disconnected due to {:?}", disconnect_reason);
                        debug!("closing gdbserver thread");
                        return self.terminate();
                    }
                    Ok(None) => {}
                    Err(_) => {
                        error!("An error occured while handling debug request {:?}", req);
                        return self.terminate();
                    }
                }
            }
            // If target is in stopped state, stay here.
            if !self.stopped {
                break;
            }
        }
        Some(self)
    }

    fn handle_request(
        &mut self,
        gba: &mut GameBoyAdvance,
        req: &mut DebuggerRequest,
    ) -> Result<Option<DisconnectReason>, TargetError<<DebuggerTarget as Target>::Error>> {
        use DebuggerRequest::*;
        match req {
            ReadRegs(regs) => {
                let mut regs = regs.lock().unwrap();
                gba.cpu.read_registers(&mut regs)?;
                trace!("Debugger requested to read regs: {:?}", regs);
                self.complete_request(None)
            }
            WriteRegs(regs) => {
                trace!("Debugger requested to write regs: {:?}", regs);
                gba.cpu.write_registers(regs)?;
                self.complete_request(None)
            }
            ReadAddrs(addr, data) => {
                let mut data = data.lock().unwrap();
                trace!(
                    "Debugger requested to read {} bytes from 0x{:08x}",
                    data.len(),
                    addr
                );
                gba.cpu.read_addrs(*addr, &mut data)?;
                self.complete_request(None)
            }
            WriteAddrs(addr, data) => {
                trace!(
                    "Debugger requested to write {} bytes at 0x{:08x}",
                    data.len(),
                    addr
                );
                gba.cpu.write_addrs(*addr, &data)?;
                self.complete_request(None)
            }
            Interrupt => {
                debug!("Ctrl-C from debugger");
                self.stopped = true;
                self.complete_request(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
            }
            Resume => {
                debug!("Resume");
                self.stopped = false;
                self.complete_request(None)
            }
            Reset => {
                debug!("Sending reset interrupt to gba");
                self.stopped = true;
                gba.cpu.reset();
                self.complete_request(Some(SingleThreadStopReason::Signal(Signal::SIGTRAP)))
            }
            SingleStep => {
                debug!("Debugger requested single step");
                self.stopped = true;
                gba.single_step();
                let _ = gba.handle_events();
                self.complete_request(Some(SingleThreadStopReason::DoneStep))
            }
            AddSwBreakpoint(addr) => {
                gba.cpu.add_breakpoint(*addr);
                self.complete_request(None)
            }
            DelSwBreakpoint(addr) => {
                gba.cpu.del_breakpoint(*addr);
                self.complete_request(None)
            }
            Disconnected(reason) => Ok(Some(*reason)),
        }
    }

    fn terminate(mut self) -> Option<DebuggerRequestHandler> {
        self.notify_stop_reason(SingleThreadStopReason::Exited(1));
        self.thread.join().unwrap();
        None
    }

    fn notify_request_complete(&mut self) {
        let (lock, cvar) = &*self.request_complete_signal;
        let mut finished = lock.lock().unwrap();
        *finished = true;
        cvar.notify_one();
        // wait for the ack
        while *finished {
            finished = cvar.wait(finished).unwrap();
        }
    }

    fn complete_request(
        &mut self,
        stop_reason: Option<SingleThreadStopReason<u32>>,
    ) -> Result<Option<DisconnectReason>, TargetError<<DebuggerTarget as Target>::Error>> {
        self.notify_request_complete();
        if let Some(stop_reason) = stop_reason {
            self.notify_stop_reason(stop_reason);
        }
        Ok(None)
    }

    pub fn notify_stop_reason(&mut self, reason: SingleThreadStopReason<u32>) {
        debug!("Notifying debugger on stop reason: {:?}", reason);
        let (lock, cvar) = &*self.stop_signal;
        let mut stop_reason = lock.lock().unwrap();
        *stop_reason = Some(reason);
        cvar.notify_one();
    }

    pub fn notify_breakpoint(&mut self, _bp: Addr) {
        self.stopped = true;
        self.notify_stop_reason(SingleThreadStopReason::SwBreak(()));
    }
}
