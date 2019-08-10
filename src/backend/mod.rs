use super::core::keypad;

mod minifb_backend;
pub use minifb_backend::MinifbBackend;

mod sdl2_backend;
pub use sdl2_backend::Sdl2Backend;

pub trait EmulatorBackend {
    fn render(&mut self, buffer: Vec<u32>);

    fn get_key_state(&mut self) -> u16;
}

pub struct DummyBackend;

impl DummyBackend {
    pub fn new() -> DummyBackend {
        DummyBackend {}
    }
}

impl EmulatorBackend for DummyBackend {
    fn get_key_state(&mut self) -> u16 {
        keypad::KEYINPUT_ALL_RELEASED
    }
    fn render(&mut self, _buffer: Vec<u32>) {}
}
