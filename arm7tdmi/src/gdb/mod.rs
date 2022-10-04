use std::io;
use std::net::{TcpListener, TcpStream};

use log::info;

mod breakpoints;
pub mod target;

// Re-export the gdbstub crate
pub extern crate gdbstub;
pub extern crate gdbstub_arch;

/// Wait for tcp connection on port
pub fn wait_for_connection(port: u16) -> io::Result<TcpStream> {
    let bind_addr = format!("0.0.0.0:{port}");
    info!("waiting for connection on {:?}", bind_addr);

    let sock = TcpListener::bind(bind_addr)?;

    // Blocks until a GDB client connects via TCP.
    // i.e: Running `target remote localhost:<port>` from the GDB prompt.
    let (stream, addr) = sock.accept()?;

    info!("gdb connected from {:?}", addr);

    Ok(stream)
}

/// Copy all bytes of `data` to `buf`.
/// Return the size of data copied.
pub fn copy_to_buf(data: &[u8], buf: &mut [u8]) -> usize {
    let len = buf.len().min(data.len());
    buf[..len].copy_from_slice(&data[..len]);
    len
}

/// Copy a range of `data` (start at `offset` with a size of `length`) to `buf`.
/// Return the size of data copied. Returns 0 if `offset >= buf.len()`.
///
/// Mainly used by qXfer:_object_:read commands.
pub fn copy_range_to_buf(data: &[u8], offset: u64, length: usize, buf: &mut [u8]) -> usize {
    let offset = offset as usize;
    if offset > data.len() {
        return 0;
    }

    let start = offset;
    let end = (offset + length).min(data.len());
    copy_to_buf(&data[start..end], buf)
}
