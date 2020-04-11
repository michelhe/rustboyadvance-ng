pub mod arm7tdmi;
pub mod cartridge;
pub mod gpu;
pub mod sound;
pub mod sysbus;
pub use sysbus::SysBus;
pub mod interrupt;
pub mod iodev;
pub use interrupt::Interrupt;
pub use interrupt::IrqBitmask;
pub mod gba;
pub use gba::GameBoyAdvance;
pub mod bus;
pub mod dma;
pub mod keypad;
pub mod timer;
pub use bus::*;

pub use super::{AudioInterface, InputInterface, VideoInterface};

#[cfg(feature = "debugger")]
use crate::debugger;

use zip;

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum GBAError {
    IO(::std::io::Error),
    CartridgeLoadError(String),
    #[cfg(feature = "debugger")]
    DebuggerError(debugger::DebuggerError),
}

impl fmt::Display for GBAError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error: {:?}", self)
    }
}

impl Error for GBAError {
    fn description(&self) -> &str {
        "emulator error"
    }
}

pub type GBAResult<T> = Result<T, GBAError>;

impl From<::std::io::Error> for GBAError {
    fn from(err: ::std::io::Error) -> GBAError {
        GBAError::IO(err)
    }
}

#[cfg(feature = "debugger")]
impl From<debugger::DebuggerError> for GBAError {
    fn from(err: debugger::DebuggerError) -> GBAError {
        GBAError::DebuggerError(err)
    }
}

impl From<zip::result::ZipError> for GBAError {
    fn from(_err: zip::result::ZipError) -> GBAError {
        GBAError::IO(::std::io::Error::from(::std::io::ErrorKind::InvalidInput))
    }
}
