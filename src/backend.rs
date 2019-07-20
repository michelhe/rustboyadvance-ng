use crate::bit::BitIndex;

use super::core::gba::GameBoyAdvance;
use super::core::ioregs::consts::*;
use super::core::keypad;

pub use super::minifb_backend::MinifbBackend;

pub trait EmulatorBackend {
    fn render(&mut self, buffer: Vec<u32>) {}

    fn get_key_state(&self) -> u16;
}

pub struct DummyBackend;

impl DummyBackend {
    fn new() -> DummyBackend {
        DummyBackend {}
    }
}

impl EmulatorBackend for DummyBackend {
    fn get_key_state(&self) -> u16 {
        keypad::KEYINPUT_ALL_RELEASED
    }
}
