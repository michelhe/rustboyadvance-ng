use std::time;

use crate::bit::BitIndex;
use std::thread;

extern crate minifb;

use minifb::{Key, Window, WindowOptions};

use super::backend::EmulatorBackend;
use crate::core::gpu::Gpu;

use super::core::keypad;

pub struct MinifbBackend {
    window: Window,
    frames_rendered: u32,
    first_frame_start: time::Instant,
}

impl MinifbBackend {
    pub fn new() -> MinifbBackend {
        let window = Window::new(
            "rustboyadvance-ng",
            Gpu::DISPLAY_WIDTH,
            Gpu::DISPLAY_HEIGHT,
            WindowOptions {
                borderless: true,
                scale: minifb::Scale::X4,
                ..Default::default()
            },
        )
        .unwrap();

        MinifbBackend {
            window: window,
            frames_rendered: 0,
            first_frame_start: time::Instant::now(),
        }
    }
}

impl EmulatorBackend for MinifbBackend {
    fn render(&mut self, buffer: Vec<u32>) {
        let now = time::Instant::now();
        if now - self.first_frame_start >= time::Duration::from_secs(1) {
            let title = format!("rustboyadvance-ng ({} fps)", self.frames_rendered);
            self.window.set_title(&title);
            self.first_frame_start = now;
            self.frames_rendered = 0;
        }
        self.window.update_with_buffer(&buffer).unwrap();
        self.frames_rendered += 1;
    }

    fn get_key_state(&self) -> u16 {
        let mut keyinput = keypad::KEYINPUT_ALL_RELEASED;
        keyinput.set_bit(keypad::Keys::Up as usize, !self.window.is_key_down(Key::Up));
        keyinput.set_bit(
            keypad::Keys::Down as usize,
            !self.window.is_key_down(Key::Down),
        );
        keyinput.set_bit(
            keypad::Keys::Left as usize,
            !self.window.is_key_down(Key::Left),
        );
        keyinput.set_bit(
            keypad::Keys::Right as usize,
            !self.window.is_key_down(Key::Right),
        );
        keyinput.set_bit(
            keypad::Keys::ButtonB as usize,
            !self.window.is_key_down(Key::Z),
        );
        keyinput.set_bit(
            keypad::Keys::ButtonA as usize,
            !self.window.is_key_down(Key::X),
        );
        keyinput.set_bit(
            keypad::Keys::Start as usize,
            !self.window.is_key_down(Key::Enter),
        );
        keyinput.set_bit(
            keypad::Keys::Select as usize,
            !self.window.is_key_down(Key::Space),
        );
        keyinput.set_bit(
            keypad::Keys::ButtonL as usize,
            !self.window.is_key_down(Key::A),
        );
        keyinput.set_bit(
            keypad::Keys::ButtonR as usize,
            !self.window.is_key_down(Key::S),
        );
        keyinput
    }
}
