use std::cell::RefCell;
use std::rc::Rc;

use std::path::Path;
use std::time;

#[macro_use]
extern crate clap;

#[macro_use]
extern crate rustboyadvance_ng;
use rustboyadvance_ng::core::keypad;
use rustboyadvance_ng::prelude::*;
use rustboyadvance_ng::util::FpsCounter;

use bit::BitIndex;
use minifb;
use minifb::{Key, Window, WindowOptions};

struct MiniFb {
    window: minifb::Window,
}

impl VideoInterface for MiniFb {
    fn render(&mut self, buffer: &[u32]) {
        self.window.update_with_buffer(buffer).unwrap();
    }
}

impl InputInterface for MiniFb {
    fn poll(&mut self) -> u16 {
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

impl AudioInterface for MiniFb {
    fn get_sample_rate(&self) -> i32 {
        0
    }
}

fn main() {
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let skip_bios = matches.occurrences_of("skip_bios") != 0;
    let no_framerate_limit = matches.occurrences_of("no_framerate_limit") != 0;

    let bios_path = Path::new(matches.value_of("bios").unwrap_or_default());
    let rom_path = Path::new(matches.value_of("game_rom").unwrap());
    let rom_name = rom_path.file_name().unwrap().to_str().unwrap();

    let bios_bin = read_bin_file(bios_path).unwrap();
    let cart = GamepakBuilder::new().file(rom_path).build().unwrap();

    let minifb = Rc::new(RefCell::new(MiniFb {
        window: Window::new(
            "rustboyadvance-ng",
            240,
            160,
            WindowOptions {
                borderless: true,
                scale: minifb::Scale::X4,
                ..Default::default()
            },
        )
        .unwrap(),
    }));

    let mut fps_counter = FpsCounter::default();
    let mut gba = GameBoyAdvance::new(
        bios_bin.into_boxed_slice(),
        cart,
        minifb.clone(),
        minifb.clone(),
        minifb.clone(),
    );

    if skip_bios {
        gba.skip_bios();
    }

    let frame_time = time::Duration::new(0, 1_000_000_000u32 / 60);
    loop {
        let start_time = time::Instant::now();

        gba.frame();

        if let Some(fps) = fps_counter.tick() {
            let title = format!("{} ({} fps)", rom_name, fps);
            // video.borrow_mut().set_window_title(&title);
            minifb.borrow_mut().window.set_title(&title);
        }

        if !no_framerate_limit {
            let time_passed = start_time.elapsed();
            let delay = frame_time.checked_sub(time_passed);
            match delay {
                None => {}
                Some(delay) => {
                    ::std::thread::sleep(delay);
                }
            };
        }
    }
}
