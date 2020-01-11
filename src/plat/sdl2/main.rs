extern crate sdl2;
use sdl2::event::Event;
use sdl2::image::{InitFlag, LoadTexture};
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::EventPump;

extern crate spin_sleep;

use std::cell::RefCell;
use std::rc::Rc;

use std::path::Path;
use std::time;

use std::process;

#[macro_use]
extern crate clap;

mod audio;
mod input;
mod video;

use audio::create_audio_player;
use input::create_input;
use video::{create_video_interface, SCALE, SCREEN_HEIGHT, SCREEN_WIDTH};

extern crate rustboyadvance_ng;
use rustboyadvance_ng::prelude::*;
use rustboyadvance_ng::util::FpsCounter;

/// Waits for the user to drag a rom file to window
fn wait_for_rom(event_pump: &mut EventPump) -> String {
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::DropFile { filename, .. } => {
                    return filename;
                }
                Event::Quit { .. } => process::exit(0),
                _ => {}
            }
        }
    }
}

fn main() {
    let mut frame_limiter = true;
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let skip_bios = matches.occurrences_of("skip_bios") != 0;

    let debug = matches.occurrences_of("debug") != 0;

    let sdl_context = sdl2::init().expect("failed to initialize sdl2");
    let mut event_pump = sdl_context.event_pump().unwrap();

    let video_subsystem = sdl_context.video().unwrap();
    let _image_context = sdl2::image::init(InitFlag::PNG | InitFlag::JPG).unwrap();
    let window = video_subsystem
        .window(
            "RustBoyAdvance",
            SCREEN_WIDTH * SCALE,
            SCREEN_HEIGHT * SCALE,
        )
        .opengl()
        .position_centered()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().accelerated().build().unwrap();

    // Display the icon as a placeholder
    canvas.set_draw_color(Color::RGB(0x80, 0x75, 0x85)); // default background color for the icon
    canvas.clear();
    let texture_creator = canvas.texture_creator();
    let icon_texture = texture_creator
        .load_texture("assets/icon.png")
        .expect("failed to load icon");
    canvas
        .copy(
            &icon_texture,
            None,
            Some(Rect::new(0, 0, SCREEN_WIDTH * SCALE, SCREEN_HEIGHT * SCALE)),
        )
        .unwrap();
    canvas.present();

    // TODO also set window icon

    let video = Rc::new(RefCell::new(create_video_interface(canvas)));
    let audio = Rc::new(RefCell::new(create_audio_player(&sdl_context)));
    let input = Rc::new(RefCell::new(create_input()));

    let bios_path = Path::new(matches.value_of("bios").unwrap_or_default());
    let bios_bin = read_bin_file(bios_path).unwrap();

    let mut rom_path = match matches.value_of("game_rom") {
        Some(path) => path.to_string(),
        _ => {
            println!("[!] Rom file missing, please drag a rom file into the emulator window...");
            wait_for_rom(&mut event_pump)
        }
    };

    let mut rom_name = Path::new(&rom_path).file_name().unwrap().to_str().unwrap();
    let cart = Cartridge::from_path(Path::new(&rom_path)).unwrap();

    let mut cpu = arm7tdmi::Core::new();
    if skip_bios {
        cpu.skip_bios();
    }
    let cpu = cpu;

    let mut gba = GameBoyAdvance::new(
        cpu,
        bios_bin,
        cart,
        video.clone(),
        audio.clone(),
        input.clone(),
    );

    if debug {
        #[cfg(rba_with_debugger)]
        {
            gba.cpu.set_verbose(true);
            let mut debugger = Debugger::new(gba);
            println!("starting debugger...");
            debugger.repl(matches.value_of("script_file")).unwrap();
            println!("ending debugger...");
            return;
        }
        #[cfg(not(rba_with_debugger))]
        {
            panic!("Please compile me with cfg(rba_with_debugger)");
        }
    }

    let mut fps_counter = FpsCounter::default();
    let frame_time = time::Duration::new(0, 1_000_000_000u32 / 60);
    'running: loop {
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
                #[cfg(rba_with_debugger)]
                Event::KeyUp {
                    keycode: Some(Keycode::F1),
                    ..
                } => {
                    let mut debugger = Debugger::new(gba);
                    println!("starting debugger...");
                    debugger.repl(matches.value_of("script_file")).unwrap();
                    gba = debugger.gba;
                    println!("ending debugger...");
                    break;
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
                Event::Quit { .. } => break 'running,
                Event::DropFile { filename, .. } => {
                    // load the new rom
                    rom_path = filename;
                    rom_name = Path::new(&rom_path).file_name().unwrap().to_str().unwrap();
                    let cart = Cartridge::from_path(Path::new(&rom_path)).unwrap();
                    let bios_bin = read_bin_file(bios_path).unwrap();

                    // create a new emulator - TODO, export to a function
                    let mut cpu = arm7tdmi::Core::new();
                    cpu.skip_bios();
                    gba = GameBoyAdvance::new(
                        cpu,
                        bios_bin,
                        cart,
                        video.clone(),
                        audio.clone(),
                        input.clone(),
                    );
                }
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
