pub(super) mod event_loop;
mod memory_map;
mod target;

use crate::{GBAResult, GameBoyAdvance};
use arm7tdmi::gdb::gdbstub::conn::ConnectionExt;
use arm7tdmi::gdb::gdbstub::stub::{DisconnectReason, GdbStub};
use arm7tdmi::gdb::wait_for_connection;
use event_loop::GbaGdbEventLoop;

pub fn start_gdb(gba: &mut GameBoyAdvance, port: u16) -> GBAResult<DisconnectReason> {
    let conn: Box<dyn ConnectionExt<Error = std::io::Error>> = Box::new(wait_for_connection(port)?);
    let gdb = GdbStub::new(conn);
    debug!("starting debug session");
    let reason = gdb.run_blocking::<GbaGdbEventLoop>(gba)?;
    Ok(reason)
}
