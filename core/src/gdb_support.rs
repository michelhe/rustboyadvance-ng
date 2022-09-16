use std::result;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type SendSync<T> = Arc<Mutex<T>>;

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
pub(crate) enum DebuggerMessage {
    ReadRegs(SendSync<ArmCoreRegs>),
    WriteRegs(ArmCoreRegs),
    ReadAddrs(Addr, SendSync<Box<[u8]>>),
    #[allow(unused)]
    WriteAddrs(Addr, Box<[u8]>),
    AddSwBreakpoint(Addr),
    DelSwBreakpoint(Addr),
    Stop,
    Resume,
    SingleStep,
    Disconnected(DisconnectReason),
}

pub struct DebuggerTarget {
    tx: Sender<DebuggerMessage>,
    operation_signal: Arc<(Mutex<bool>, Condvar)>,
    stop_signal: Arc<(Mutex<SingleThreadStopReason<u32>>, Condvar)>,
    memory_map: String,
}

impl DebuggerTarget {
    #[inline]
    pub fn wait_for_operation(&mut self) {
        let (lock, cvar) = &*self.operation_signal;
        let mut finished = lock.lock().unwrap();
        while !*finished {
            finished = cvar.wait(finished).unwrap();
        }
        *finished = false;
    }
}

pub(crate) struct DebuggerState {
    rx: Receiver<DebuggerMessage>,
    operation_signal: Arc<(Mutex<bool>, Condvar)>,
    stop_signal: Arc<(Mutex<SingleThreadStopReason<u32>>, Condvar)>,
    thread: JoinHandle<()>,
    stopped: bool,
}

impl DebuggerState {
    pub fn handle_message(
        mut self,
        gba: &mut GameBoyAdvance,
        should_stop: &mut bool,
    ) -> Result<Option<DebuggerState>, TargetError<<DebuggerTarget as Target>::Error>> {
        if self.thread.is_finished() {
            warn!("gdb server thread unexpectdly died");
            *should_stop = true;
            self.thread.join().unwrap();
            return Ok(None);
        }
        if let Ok(msg) = self.rx.try_recv() {
            use DebuggerMessage::*;
            let mut result = match msg {
                ReadRegs(regs) => {
                    let mut regs = regs.lock().unwrap();
                    gba.cpu.read_registers(&mut regs)?;
                    debug!("Debugger requested to read regs: {:?}", regs);
                    Ok(Some(self))
                }
                WriteRegs(regs) => {
                    debug!("Debugger requested to write regs: {:?}", regs);
                    gba.cpu.write_registers(&regs)?;
                    Ok(Some(self))
                }
                ReadAddrs(addr, data) => {
                    let mut data = data.lock().unwrap();
                    debug!(
                        "Debugger requested to read {} bytes from 0x{:08x}",
                        data.len(),
                        addr
                    );
                    gba.cpu.read_addrs(addr, &mut data)?;
                    Ok(Some(self))
                }
                WriteAddrs(addr, data) => {
                    debug!(
                        "Debugger requested to write {} bytes at 0x{:08x}",
                        data.len(),
                        addr
                    );
                    gba.cpu.write_addrs(addr, &data)?;
                    Ok(Some(self))
                }
                Stop => {
                    debug!("Debugger requested stopped");
                    self.stopped = true;
                    Ok(Some(self))
                }
                Resume => {
                    debug!("Debugger requested resume");
                    self.stopped = false;
                    Ok(Some(self))
                }
                SingleStep => {
                    debug!("Debugger requested single step");
                    gba.run::<true>(1);
                    self.notify_stop_reason(SingleThreadStopReason::DoneStep);
                    self.stopped = true;
                    Ok(Some(self))
                }
                AddSwBreakpoint(addr) => {
                    gba.cpu.add_breakpoint(addr);
                    Ok(Some(self))
                }
                DelSwBreakpoint(addr) => {
                    gba.cpu.del_breakpoint(addr);
                    Ok(Some(self))
                }
                Disconnected(reason) => {
                    debug!("Debugger disconnected due to {:?}", reason);
                    debug!("closing gdbserver thread");
                    self.thread.join().unwrap();
                    Ok(None)
                }
            };
            if let Ok(Some(result)) = &mut result {
                let (lock, cvar) = &*result.operation_signal;
                let mut finished = lock.lock().unwrap();
                *finished = true;
                cvar.notify_one();
                *should_stop = result.stopped;
            } else {
                *should_stop = true;
            }
            result
        } else {
            *should_stop = self.stopped;
            Ok(Some(self))
        }
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
