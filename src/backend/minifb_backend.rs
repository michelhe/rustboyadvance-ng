use std::time;

use crate::bit::BitIndex;

extern crate minifb;

use minifb::{Key, Window, WindowOptions};

use super::EmulatorBackend;
use crate::core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::core::keypad;

pub struct MinifbBackend {
    window: Window,
    frames_rendered: u32,
    first_frame_start: time::Instant,
}

impl MinifbBackend {
    pub fn new() -> MinifbBackend {
        let window = Window::new(
            "rustboyadvance-ng",
            DISPLAY_WIDTH,
            DISPLAY_HEIGHT,
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
    fn render(&mut self, buffer: &[u32]) {
        self.frames_rendered += 1;
        if self.first_frame_start.elapsed() >= time::Duration::from_secs(1) {
            let title = format!("rustboyadvance-ng ({} fps)", self.frames_rendered);
            self.window.set_title(&title);
            self.first_frame_start = time::Instant::now();
            self.frames_rendered = 0;
        }
        self.window.update_with_buffer(buffer).unwrap();
    }

    fn get_key_state(&mut self) -> u16 {
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
