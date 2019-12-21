#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(exclusive_range_pattern)]

#[macro_use]
extern crate enum_primitive_derive;
extern crate num;
extern crate num_traits;

extern crate bit;
#[macro_use]
extern crate bitfield;
#[macro_use]
extern crate bitflags;

extern crate byteorder;

extern crate rustyline;

extern crate nom;

extern crate ansi_term;
extern crate colored; // not needed in Rust 2018

extern crate zip;

#[macro_use]
pub mod util;
pub mod core;
pub mod debugger;
pub mod disass;

pub trait VideoInterface {
    fn render(&mut self, buffer: &[u32]);
}

pub trait AudioInterface {
    fn get_sample_rate(&self) -> u32;
}

pub trait InputInterface {
    fn poll(&mut self) -> u16;
}

pub mod prelude {
    pub use super::core::arm7tdmi;
    pub use super::core::cartridge::Cartridge;
    pub use super::core::{GBAError, GBAResult, GameBoyAdvance};
    pub use super::debugger::Debugger;
    pub use super::util::read_bin_file;
    pub use super::{AudioInterface, InputInterface, VideoInterface};
}
