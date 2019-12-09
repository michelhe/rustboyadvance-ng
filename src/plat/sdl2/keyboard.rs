use sdl2::keyboard::Keycode;
use sdl2::{event::Event, EventPump};

use rustboyadvance_ng::core::keypad as gba_keypad;
use rustboyadvance_ng::InputInterface;

extern crate bit;
use bit::BitIndex;

pub struct Sdl2Input {
    event_pump: EventPump,
    keyinput: u16,
}

impl InputInterface for Sdl2Input {
    fn poll(&mut self) -> u16 {
        for event in self.event_pump.poll_iter() {
            match event {
                Event::KeyDown {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_keypad(keycode) {
                        self.keyinput.set_bit(key as usize, false);
                    }
                }
                Event::KeyUp {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_keypad(keycode) {
                        self.keyinput.set_bit(key as usize, true);
                    }
                }
                Event::Quit { .. } => panic!("quit!"),
                _ => {}
            }
        }
        self.keyinput
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
        Keycode::Space => Some(gba_keypad::Keys::Select),
        Keycode::A => Some(gba_keypad::Keys::ButtonL),
        Keycode::S => Some(gba_keypad::Keys::ButtonR),
        _ => None,
    }
}

pub fn create_keyboard(sdl: &sdl2::Sdl) -> Sdl2Input {
    Sdl2Input {
        event_pump: sdl.event_pump().unwrap(),
        keyinput: gba_keypad::KEYINPUT_ALL_RELEASED,
    }
}
