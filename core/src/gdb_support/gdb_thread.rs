use std::sync::{Arc, Condvar, Mutex};

use arm7tdmi::{
    gdb::wait_for_connection,
    gdbstub::{
        conn::ConnectionExt,
        stub::GdbStub,
    },
};

use crate::{GBAError, GameBoyAdvance};

use super::target::DebuggerTarget;
use super::{event_loop::DebuggerEventLoop, DebuggerRequestHandler};

/// Starts a gdbserver thread
pub(crate) fn start_gdb_server_thread(
    gba: &mut GameBoyAdvance,
    port: u16,
) -> Result<DebuggerRequestHandler, GBAError> {
    let (tx, rx) = crossbeam::channel::unbounded();
    let request_complete_signal = Arc::new((Mutex::new(false), Condvar::new()));
    let stop_signal = Arc::new((Mutex::new(None), Condvar::new()));
    let stop_signal_2 = stop_signal.clone();
    let request_complete_signal_2 = request_complete_signal.clone();
    let memory_map = gba.sysbus.generate_memory_map_xml().unwrap();

    let conn = wait_for_connection(port)?;
    let thread = std::thread::spawn(move || {
        debug!("starting GDB Server thread");
        let conn: Box<dyn ConnectionExt<Error = std::io::Error>> = Box::new(conn);

        let mut target =
            DebuggerTarget::new(tx, request_complete_signal_2, stop_signal_2, memory_map);
        let gdbserver = GdbStub::new(conn);
        let disconnect_reason = gdbserver
            .run_blocking::<DebuggerEventLoop>(&mut target)
            .map_err(|e| e.to_string())
            .unwrap();
        target.disconnect(disconnect_reason);
    });

    let debugger = DebuggerRequestHandler {
        rx,
        request_complete_signal,
        stop_signal,
        thread,
        stopped: true,
    };
    Ok(debugger)
}
