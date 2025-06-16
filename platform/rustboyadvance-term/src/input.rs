use crossterm::event::KeyCode;
use rustboyadvance_core::keypad as gba_keypad;

use bit;
use bit::BitIndex;

pub fn on_keyboard_key_down(key_state: &mut u16, scancode: KeyCode) {
    if let Some(key) = scancode_to_keypad(scancode) {
        key_state.set_bit(key as usize, false);
    }
}

pub fn on_keyboard_key_up(key_state: &mut u16, scancode: KeyCode) {
    if let Some(key) = scancode_to_keypad(scancode) {
        key_state.set_bit(key as usize, true);
    }
}

fn scancode_to_keypad(scancode: KeyCode) -> Option<gba_keypad::Keys> {
    match scancode {
        KeyCode::Up => Some(gba_keypad::Keys::Up),
        KeyCode::Down => Some(gba_keypad::Keys::Down),
        KeyCode::Left => Some(gba_keypad::Keys::Left),
        KeyCode::Right => Some(gba_keypad::Keys::Right),
        KeyCode::Char('z') => Some(gba_keypad::Keys::ButtonB),
        KeyCode::Char('x') => Some(gba_keypad::Keys::ButtonA),
        KeyCode::Enter => Some(gba_keypad::Keys::Start),
        KeyCode::Backspace => Some(gba_keypad::Keys::Select),
        KeyCode::Char('a') => Some(gba_keypad::Keys::ButtonL),
        KeyCode::Char('s') => Some(gba_keypad::Keys::ButtonR),
        _ => None,
    }
}
