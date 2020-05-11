use sdl2::controller::Axis;
use sdl2::controller::Button;
use sdl2::keyboard::Scancode;

use rustboyadvance_core::keypad as gba_keypad;
use rustboyadvance_core::InputInterface;

use bit;
use bit::BitIndex;

pub struct Sdl2Input {
    keyinput: u16,
    axis_keyinput: u16,
}

impl InputInterface for Sdl2Input {
    fn poll(&mut self) -> u16 {
        !(!self.keyinput | !self.axis_keyinput)
    }
}

impl Sdl2Input {
    pub fn on_keyboard_key_down(&mut self, scancode: Scancode) {
        if let Some(key) = scancode_to_keypad(scancode) {
            self.keyinput.set_bit(key as usize, false);
        }
    }

    pub fn on_keyboard_key_up(&mut self, scancode: Scancode) {
        if let Some(key) = scancode_to_keypad(scancode) {
            self.keyinput.set_bit(key as usize, true);
        }
    }

    pub fn on_controller_button_down(&mut self, button: Button) {
        if let Some(key) = controller_button_to_keypad(button) {
            self.keyinput.set_bit(key as usize, false);
        }
    }

    pub fn on_controller_button_up(&mut self, button: Button) {
        if let Some(key) = controller_button_to_keypad(button) {
            self.keyinput.set_bit(key as usize, true);
        }
    }

    pub fn on_axis_motion(&mut self, axis: Axis, val: i16) {
        use gba_keypad::Keys as GbaKeys;
        let keys = match axis {
            Axis::LeftX => (GbaKeys::Left, GbaKeys::Right),
            Axis::LeftY => (GbaKeys::Up, GbaKeys::Down),
            Axis::TriggerLeft => (GbaKeys::ButtonL, GbaKeys::ButtonL),
            Axis::TriggerRight => (GbaKeys::ButtonR, GbaKeys::ButtonR),
            _ => {
                return;
            }
        };

        // Axis motion is an absolute value in the range
        // [-32768, 32767]. Let's simulate a very rough dead
        // zone to ignore spurious events.
        let dead_zone = 10_000;
        if val > dead_zone || val < -dead_zone {
            let key = if val < 0 { keys.0 } else { keys.1 };
            self.axis_keyinput.set_bit(key as usize, false);
        } else {
            self.axis_keyinput.set_bit(keys.0 as usize, true);
            self.axis_keyinput.set_bit(keys.1 as usize, true);
        }
    }
}

fn scancode_to_keypad(scancode: Scancode) -> Option<gba_keypad::Keys> {
    match scancode {
        Scancode::Up => Some(gba_keypad::Keys::Up),
        Scancode::Down => Some(gba_keypad::Keys::Down),
        Scancode::Left => Some(gba_keypad::Keys::Left),
        Scancode::Right => Some(gba_keypad::Keys::Right),
        Scancode::Z => Some(gba_keypad::Keys::ButtonB),
        Scancode::X => Some(gba_keypad::Keys::ButtonA),
        Scancode::Return => Some(gba_keypad::Keys::Start),
        Scancode::Backspace => Some(gba_keypad::Keys::Select),
        Scancode::A => Some(gba_keypad::Keys::ButtonL),
        Scancode::S => Some(gba_keypad::Keys::ButtonR),
        _ => None,
    }
}

fn controller_button_to_keypad(button: Button) -> Option<gba_keypad::Keys> {
    match button {
        Button::DPadUp => Some(gba_keypad::Keys::Up),
        Button::DPadDown => Some(gba_keypad::Keys::Down),
        Button::DPadLeft => Some(gba_keypad::Keys::Left),
        Button::DPadRight => Some(gba_keypad::Keys::Right),
        Button::A => Some(gba_keypad::Keys::ButtonB), // A and B are swapped compared to the SDL layout
        Button::B => Some(gba_keypad::Keys::ButtonA),
        Button::Start => Some(gba_keypad::Keys::Start),
        Button::Back => Some(gba_keypad::Keys::Select),
        Button::LeftShoulder => Some(gba_keypad::Keys::ButtonL),
        Button::RightShoulder => Some(gba_keypad::Keys::ButtonR),
        _ => None,
    }
}

pub fn create_input() -> Sdl2Input {
    Sdl2Input {
        keyinput: gba_keypad::KEYINPUT_ALL_RELEASED,
        axis_keyinput: gba_keypad::KEYINPUT_ALL_RELEASED,
    }
}
