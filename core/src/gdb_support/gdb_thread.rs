use std::sync::{Arc, Condvar, Mutex};

use arm7tdmi::{
    gdb::wait_for_connection,
    gdbstub::{
        common::Signal,
        conn::ConnectionExt,
        stub::{GdbStub, SingleThreadStopReason},
    },
};

use crate::{GBAError, GameBoyAdvance};

use super::{event_loop::DebuggerEventLoop, DebuggerMessage, DebuggerState, DebuggerTarget};

/// Starts a gdbserver thread
pub(crate) fn start_gdb_server_thread(
    gba: &mut GameBoyAdvance,
    port: u16,
) -> Result<DebuggerState, GBAError> {
    let (tx, rx) = crossbeam::channel::unbounded();
    let operation_signal = Arc::new((Mutex::new(false), Condvar::new()));
    let stop_signal = Arc::new((
        Mutex::new(SingleThreadStopReason::Signal(Signal::SIGINT)),
        Condvar::new(),
    ));
    let stop_signal_2 = stop_signal.clone();
    let operation_signal_2 = operation_signal.clone();
    let memory_map = gba.sysbus.generate_memory_map_xml().unwrap();

    let conn = wait_for_connection(port)?;
    let thread = std::thread::spawn(move || {
        debug!("starting GDB Server thread");
        let conn: Box<dyn ConnectionExt<Error = std::io::Error>> = Box::new(conn);

        let mut target = DebuggerTarget {
            tx,
            operation_signal: operation_signal_2,
            stop_signal: stop_signal_2,
            memory_map,
        };
        let gdbserver = GdbStub::new(conn);
        let disconnect_reason = gdbserver
            .run_blocking::<DebuggerEventLoop>(&mut target)
            .map_err(|e| e.to_string())
            .unwrap();
        target
            .tx
            .send(DebuggerMessage::Disconnected(disconnect_reason))
            .unwrap();
    });

    let mut debugger = DebuggerState {
        rx,
        operation_signal,
        stop_signal,
        thread,
        stopped: true,
    };
    debugger.notify_stop_reason(SingleThreadStopReason::Signal(Signal::SIGINT));

    Ok(debugger)
}
