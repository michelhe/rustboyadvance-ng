extern crate sdl2;

use std::cell::RefCell;
use std::rc::Rc;

use std::path::Path;
use std::time;

#[macro_use]
extern crate clap;

mod audio;
mod keyboard;
mod video;

use audio::{create_audio_player, Sdl2AudioPlayer};
use keyboard::{create_keyboard, Sdl2Input};
use video::{create_video_interface, Sdl2Video};

#[macro_use]
extern crate rustboyadvance_ng;
use rustboyadvance_ng::prelude::*;
use rustboyadvance_ng::util::FpsCounter;

fn main() {
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let skip_bios = matches.occurrences_of("skip_bios") != 0;
    let no_framerate_limit = matches.occurrences_of("no_framerate_limit") != 0;
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
    let keyboard = Rc::new(RefCell::new(create_keyboard(&sdl_context)));

    let mut fps_counter = FpsCounter::default();
    let mut gba: GameBoyAdvance<Sdl2Video, Sdl2AudioPlayer, Sdl2Input> =
        GameBoyAdvance::new(cpu, bios_bin, cart, video.clone(), audio.clone(), keyboard.clone());

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

            gba.frame();

            if let Some(fps) = fps_counter.tick() {
                let title = format!("{} ({} fps)", rom_name, fps);
                video.borrow_mut().set_window_title(&title);
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
}
