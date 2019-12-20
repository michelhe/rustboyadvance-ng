extern crate sdl2;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

extern crate spin_sleep;

use std::cell::RefCell;
use std::rc::Rc;

use std::path::Path;
use std::time;

#[macro_use]
extern crate clap;

mod audio;
mod input;
mod video;

use audio::{create_audio_player, Sdl2AudioPlayer};
use input::{create_input, Sdl2Input};
use video::{create_video_interface, Sdl2Video};

extern crate rustboyadvance_ng;
use rustboyadvance_ng::prelude::*;
use rustboyadvance_ng::util::FpsCounter;

fn main() {
    let mut frame_limiter = true;
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let skip_bios = matches.occurrences_of("skip_bios") != 0;
    let debug = matches.occurrences_of("debug") != 0;

    let bios_path = Path::new(matches.value_of("bios").unwrap_or_default());
    let rom_path = Path::new(matches.value_of("game_rom").unwrap());
    let rom_name = rom_path.file_name().unwrap().to_str().unwrap();

    let bios_bin = read_bin_file(bios_path).unwrap();
    let cart = Cartridge::from_path(rom_path).unwrap();
    let mut cpu = arm7tdmi::Core::new();
    if skip_bios {
        cpu.skip_bios();
    }
    let cpu = cpu;

    let sdl_context = sdl2::init().unwrap();
    let video = Rc::new(RefCell::new(create_video_interface(&sdl_context)));
    let audio = Rc::new(RefCell::new(create_audio_player(&sdl_context)));
    let input = Rc::new(RefCell::new(create_input()));

    let mut fps_counter = FpsCounter::default();
    let mut gba: GameBoyAdvance<Sdl2Video, Sdl2AudioPlayer, Sdl2Input> = GameBoyAdvance::new(
        cpu,
        bios_bin,
        cart,
        video.clone(),
        audio.clone(),
        input.clone(),
    );

    let mut event_pump = sdl_context.event_pump().unwrap();

    if debug {
        gba.cpu.set_verbose(true);
        let mut debugger = Debugger::new(gba);
        println!("starting debugger...");
        debugger.repl(matches.value_of("script_file")).unwrap();
        println!("ending debugger...");
    } else {
        let frame_time = time::Duration::new(0, 1_000_000_000u32 / 60);
        loop {
            let start_time = time::Instant::now();

            for event in event_pump.poll_iter() {
                match event {
                    Event::KeyDown {
                        keycode: Some(Keycode::Space),
                        ..
                    } => {
                        frame_limiter = false;
                    }
                    Event::KeyUp {
                        keycode: Some(Keycode::Space),
                        ..
                    } => {
                        frame_limiter = true;
                    }
                    Event::KeyDown {
                        keycode: Some(keycode),
                        ..
                    } => {
                        input.borrow_mut().on_keyboard_key_down(keycode);
                    }
                    Event::KeyUp {
                        keycode: Some(keycode),
                        ..
                    } => {
                        input.borrow_mut().on_keyboard_key_up(keycode);
                    }
                    Event::Quit { .. } => panic!("quit!"),
                    _ => {}
                }
            }

            gba.frame();

            if let Some(fps) = fps_counter.tick() {
                let title = format!("{} ({} fps)", rom_name, fps);
                video.borrow_mut().set_window_title(&title);
            }

            if frame_limiter {
                let time_passed = start_time.elapsed();
                let delay = frame_time.checked_sub(time_passed);
                match delay {
                    None => {}
                    Some(delay) => {
                        spin_sleep::sleep(delay);
                    }
                };
            }
        }
    }
}
