#[macro_use]
extern crate serde;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate debug_stub_derive;

#[macro_use]
extern crate enum_primitive_derive;

#[macro_use]
extern crate bitfield;
#[macro_use]
extern crate bitflags;

#[macro_use]
extern crate log;

#[macro_use]
extern crate hex_literal;

#[macro_use]
extern crate smart_default;

extern crate cfg_if;

use std::error::Error;
use std::fmt;

pub use arm7tdmi;
pub use arm7tdmi::disass;
mod bios;
pub mod cartridge;
pub mod gpu;
mod sched;
pub mod sound;
pub mod sysbus;
pub use sysbus::SysBus;
pub mod interrupt;
pub mod iodev;
pub use interrupt::Interrupt;
pub use interrupt::SharedInterruptFlags;
pub mod gba;
pub use gba::GameBoyAdvance;
pub mod dma;
pub mod gdb_support;
pub mod keypad;
mod mgba_debug;
pub(crate) mod overrides;
pub mod timer;

use arm7tdmi::gdb::gdbstub::stub::GdbStubError;

#[cfg(feature = "debugger")]
pub mod debugger;

#[derive(Debug)]
pub enum GBAError {
    IO(::std::io::Error),
    CartridgeLoadError(String),
    #[cfg(feature = "debugger")]
    DebuggerError(debugger::DebuggerError),
    GdbError(String),
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

impl From<GdbStubError<(), std::io::Error>> for GBAError {
    fn from(err: GdbStubError<(), std::io::Error>) -> Self {
        GBAError::GdbError(err.to_string())
    }
}

pub mod prelude {
    pub use super::cartridge::{Cartridge, GamepakBuilder};
    #[cfg(feature = "debugger")]
    pub use super::debugger::Debugger;
    pub use super::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
    pub use super::sound::interface::{
        AudioInterface, DynAudioInterface, NullAudio, SimpleAudioInterface,
    };
    pub use super::{GBAError, GBAResult, GameBoyAdvance};
    pub use arm7tdmi;
    pub use arm7tdmi::memory::{Addr, BusIO, MemoryAccess, MemoryAccessWidth, MemoryInterface};
    pub use rustboyadvance_utils::{read_bin_file, write_bin_file};
}
