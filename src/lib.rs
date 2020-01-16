#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(exclusive_range_pattern)]

#[macro_use]
extern crate serde;
extern crate bincode;

#[macro_use]
extern crate debug_stub_derive;

#[macro_use]
extern crate enum_primitive_derive;
extern crate num;
extern crate num_traits;

extern crate bit;
#[macro_use]
extern crate bitfield;
#[macro_use]
extern crate bitflags;
extern crate bit_set;

extern crate byteorder;

#[cfg(feature = "debugger")]
extern crate rustyline;

#[cfg(feature = "debugger")]
extern crate nom;

extern crate ansi_term;
extern crate colored; // not needed in Rust 2018

extern crate zip;

#[macro_use]
pub mod util;
pub mod core;
pub mod disass;

#[cfg(feature = "debugger")]
pub mod debugger;

pub trait VideoInterface {
    fn render(&mut self, buffer: &[u32]);
}

pub type StereoSample = (i16, i16);

pub trait AudioInterface {
    fn get_sample_rate(&self) -> i32;

    #[allow(unused_variables)]
    fn push_sample(&mut self, samples: StereoSample) {}
}

pub trait InputInterface {
    fn poll(&mut self) -> u16;
}

pub mod prelude {
    pub use super::core::arm7tdmi;
    pub use super::core::cartridge::Cartridge;
    pub use super::core::{GBAError, GBAResult, GameBoyAdvance};
    #[cfg(feature = "debugger")]
    pub use super::debugger::Debugger;
    pub use super::util::read_bin_file;
    pub use super::{AudioInterface, InputInterface, VideoInterface};
}
