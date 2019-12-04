extern crate sdl2;
use sdl2::Sdl;

use std::path::Path;
use std::time;

#[macro_use]
extern crate clap;

mod audio;
mod keyboard;
mod video;

use keyboard::{create_keyboard, PlatformSdl2_Keyboard};
use video::{create_video_interface, PlatformSdl2_VideoInterface};

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
    let mut video = create_video_interface(&sdl_context);
    let mut keyboard = create_keyboard(&sdl_context);

    let mut fps_counter = FpsCounter::default();
    let mut gba = GameBoyAdvance::new(cpu, bios_bin, cart);

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

            gba.frame(&mut video, &mut keyboard);
            if let Some(fps) = fps_counter.tick() {
                let title = format!("{} ({} fps)", rom_name, fps);
                video.set_window_title(&title);
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
