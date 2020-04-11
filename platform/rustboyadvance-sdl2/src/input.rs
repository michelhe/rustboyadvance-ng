use sdl2::keyboard::Keycode;

use rustboyadvance_core::core::keypad as gba_keypad;
use rustboyadvance_core::InputInterface;

use bit;
use bit::BitIndex;

pub struct Sdl2Input {
    keyinput: u16,
}

impl InputInterface for Sdl2Input {
    fn poll(&mut self) -> u16 {
        self.keyinput
    }
}

impl Sdl2Input {
    pub fn on_keyboard_key_down(&mut self, keycode: Keycode) {
        if let Some(key) = keycode_to_keypad(keycode) {
            self.keyinput.set_bit(key as usize, false);
        }
    }
    pub fn on_keyboard_key_up(&mut self, keycode: Keycode) {
        if let Some(key) = keycode_to_keypad(keycode) {
            self.keyinput.set_bit(key as usize, true);
        }
    }
}

fn keycode_to_keypad(keycode: Keycode) -> Option<gba_keypad::Keys> {
    match keycode {
        Keycode::Up => Some(gba_keypad::Keys::Up),
        Keycode::Down => Some(gba_keypad::Keys::Down),
        Keycode::Left => Some(gba_keypad::Keys::Left),
        Keycode::Right => Some(gba_keypad::Keys::Right),
        Keycode::Z => Some(gba_keypad::Keys::ButtonB),
        Keycode::X => Some(gba_keypad::Keys::ButtonA),
        Keycode::Return => Some(gba_keypad::Keys::Start),
        Keycode::Backspace => Some(gba_keypad::Keys::Select),
        Keycode::A => Some(gba_keypad::Keys::ButtonL),
        Keycode::S => Some(gba_keypad::Keys::ButtonR),
        _ => None,
    }
}

pub fn create_input() -> Sdl2Input {
    Sdl2Input {
        keyinput: gba_keypad::KEYINPUT_ALL_RELEASED,
    }
}
