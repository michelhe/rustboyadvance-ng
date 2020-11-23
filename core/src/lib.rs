#[macro_use]
extern crate serde;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate debug_stub_derive;

#[macro_use]
extern crate enum_primitive_derive;
use num;
use num_traits;

use bit;
#[macro_use]
extern crate bitfield;
#[macro_use]
extern crate bitflags;

use byteorder;

#[macro_use]
extern crate log;

#[macro_use]
extern crate hex_literal;

#[macro_use]
extern crate smart_default;

extern crate cfg_if;

use zip;

use std::error::Error;
use std::fmt;

#[macro_use]
pub mod util;
pub mod arm7tdmi;
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
pub mod bus;
pub mod dma;
pub mod keypad;
pub mod timer;
pub use bus::*;
mod mgba_debug;
pub(crate) mod overrides;

#[cfg(feature = "gdb")]
pub mod gdb;

#[cfg(feature = "debugger")]
pub mod debugger;

#[cfg(not(feature = "no_video_interface"))]
pub trait VideoInterface {
    #[allow(unused_variables)]
    fn render(&mut self, buffer: &[u32]) {}
}

pub type StereoSample<T> = (T, T);

pub trait AudioInterface {
    fn get_sample_rate(&self) -> i32 {
        44100
    }

    /// Pushes a stereo sample into the audio device
    /// Sample should be normilized to siged 16bit values
    /// Note: It is not guarentied that the sample will be played
    #[allow(unused_variables)]
    fn push_sample(&mut self, samples: &[i16]) {}
}

pub trait InputInterface {
    fn poll(&mut self) -> u16 {
        keypad::KEYINPUT_ALL_RELEASED
    }
}

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

pub mod prelude {
    pub use super::arm7tdmi;
    pub use super::cartridge::{Cartridge, GamepakBuilder};
    #[cfg(feature = "debugger")]
    pub use super::debugger::Debugger;
    pub use super::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
    pub use super::util::{read_bin_file, write_bin_file};
    pub use super::Bus;
    #[cfg(not(feature = "no_video_interface"))]
    pub use super::VideoInterface;
    pub use super::{AudioInterface, InputInterface, StereoSample};
    pub use super::{GBAError, GBAResult, GameBoyAdvance};
}
