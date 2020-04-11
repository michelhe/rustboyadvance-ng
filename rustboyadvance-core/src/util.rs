use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::time;

use crate::core::GameBoyAdvance;
#[cfg(feature = "gdb")]
use gdbstub;
#[cfg(feature = "gdb")]
use gdbstub::GdbStub;
use std::fmt;
#[cfg(feature = "gdb")]
use std::net::TcpListener;
use std::net::ToSocketAddrs;

pub fn spawn_and_run_gdb_server<A: ToSocketAddrs + fmt::Display>(
    target: &mut GameBoyAdvance,
    addr: A,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "gdb")]
    {
        info!("spawning gdbserver, listening on {}", addr);

        let sock = TcpListener::bind(addr)?;
        let (stream, addr) = sock.accept()?;

        info!("got connection from {}", addr);

        let mut gdb = GdbStub::new(stream);
        let result = match gdb.run(target) {
            Ok(state) => {
                info!("Disconnected from GDB. Target state: {:?}", state);
                Ok(())
            }
            Err(gdbstub::Error::TargetError(e)) => Err(e),
            Err(e) => return Err(e.into()),
        };

        info!("Debugger session ended, result={:?}", result);
    }
    #[cfg(not(feature = "gdb"))]
    {
        error!("failed. please compile me with 'gdb' feature")
    }

    Ok(())
}

pub fn read_bin_file(filename: &Path) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn write_bin_file(filename: &Path, data: &Vec<u8>) -> io::Result<()> {
    let mut f = File::create(filename)?;
    f.write_all(data)?;

    Ok(())
}

pub struct FpsCounter {
    count: u32,
    timer: time::Instant,
}

const SECOND: time::Duration = time::Duration::from_secs(1);

impl Default for FpsCounter {
    fn default() -> FpsCounter {
        FpsCounter {
            count: 0,
            timer: time::Instant::now(),
        }
    }
}

impl FpsCounter {
    pub fn tick(&mut self) -> Option<u32> {
        self.count += 1;
        if self.timer.elapsed() >= SECOND {
            let fps = self.count;
            self.timer = time::Instant::now();
            self.count = 0;
            Some(fps)
        } else {
            None
        }
    }
}

#[macro_export]
macro_rules! index2d {
    ($x:expr, $y:expr, $w:expr) => {
        $w * $y + $x
    };
    ($t:ty, $x:expr, $y:expr, $w:expr) => {
        (($w as $t) * ($y as $t) + ($x as $t)) as $t
    };
}

#[allow(unused_macros)]
macro_rules! host_breakpoint {
    () => {
        #[cfg(debug_assertions)]
        unsafe {
            ::std::intrinsics::breakpoint()
        }
    };
}
