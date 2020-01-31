extern crate sdl2;
use sdl2::event::Event;
use sdl2::image::{InitFlag, LoadTexture};
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::EventPump;

extern crate bytesize;
extern crate spin_sleep;

use std::cell::RefCell;
use std::rc::Rc;

use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time;

#[macro_use]
extern crate clap;

#[macro_use]
extern crate log;
extern crate flexi_logger;
use flexi_logger::*;

mod audio;
mod input;
mod video;

use audio::create_audio_player;
use input::create_input;
use video::{create_video_interface, SCREEN_HEIGHT, SCREEN_WIDTH};

extern crate rustboyadvance_ng;
use rustboyadvance_ng::prelude::*;
use rustboyadvance_ng::util::FpsCounter;

const LOG_DIR: &str = ".logs";

fn get_savestate_path(rom_filename: &Path) -> PathBuf {
    rom_filename.with_extension("savestate")
}

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir(LOG_DIR);
    flexi_logger::Logger::with_env_or_str("info")
        .log_to_file()
        .directory(LOG_DIR)
        .duplicate_to_stderr(Duplicate::Debug)
        .format_for_files(default_format)
        .format_for_stderr(colored_default_format)
        .start()
        .unwrap();

    let mut frame_limiter = true;
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let skip_bios = matches.occurrences_of("skip_bios") != 0;

    let debug = matches.occurrences_of("debug") != 0;

    info!("Initializing SDL2 context");
    let sdl_context = sdl2::init().expect("failed to initialize sdl2");
    let mut event_pump = sdl_context.event_pump()?;

    let video_subsystem = sdl_context.video()?;
    let _image_context = sdl2::image::init(InitFlag::PNG | InitFlag::JPG)?;
    let window = video_subsystem
        .window("RustBoyAdvance", SCREEN_WIDTH * 3, SCREEN_HEIGHT * 3)
        .opengl()
        .position_centered()
        .resizable()
        .build()?;
    let mut canvas = window.into_canvas().accelerated().build()?;

    canvas.set_logical_size(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)?;

    // Display the icon as a placeholder
    canvas.set_draw_color(Color::RGB(0x40, 0x22, 0x20)); // default background color for the icon
    canvas.clear();
    let texture_creator = canvas.texture_creator();
    let icon_texture = texture_creator
        .load_texture("assets/icon.png")
        .expect("failed to load icon");
    canvas.copy(&icon_texture, None, None).unwrap();
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
            info!("[!] Rom file missing, please drag a rom file into the emulator window...");
            wait_for_rom(&mut event_pump)
        }
    };

    let mut savestate_path = get_savestate_path(&Path::new(&rom_path));

    let mut rom_name = Path::new(&rom_path).file_name().unwrap().to_str().unwrap();
    let gamepak = GamepakBuilder::new().file(Path::new(&rom_path)).build()?;

    let mut gba = GameBoyAdvance::new(
        arm7tdmi::Core::new(),
        bios_bin,
        gamepak,
        video.clone(),
        audio.clone(),
        input.clone(),
    );

    if skip_bios {
        gba.skip_bios();
    }

    if debug {
        #[cfg(feature = "debugger")]
        {
            gba.cpu.set_verbose(true);
            let mut debugger = Debugger::new(gba);
            info!("starting debugger...");
            debugger.repl(matches.value_of("script_file")).unwrap();
            info!("ending debugger...");
            return;
        }
        #[cfg(not(feature = "debugger"))]
        {
            panic!("Please compile me with 'debugger' feature");
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
                #[cfg(feature = "debugger")]
                Event::KeyUp {
                    keycode: Some(Keycode::F1),
                    ..
                } => {
                    let mut debugger = Debugger::new(gba);
                    info!("starting debugger...");
                    debugger.repl(matches.value_of("script_file")).unwrap();
                    gba = debugger.gba;
                    info!("ending debugger...");
                    break;
                }
                Event::KeyUp {
                    keycode: Some(Keycode::F5),
                    ..
                } => {
                    info!("Saving state ...");
                    let save = gba.save_state()?;
                    write_bin_file(&savestate_path, &save)?;
                    info!(
                        "Saved to {:?} ({})",
                        savestate_path,
                        bytesize::ByteSize::b(save.len() as u64)
                    );
                }
                Event::KeyUp {
                    keycode: Some(Keycode::F9),
                    ..
                } => {
                    if savestate_path.is_file() {
                        let save = read_bin_file(&savestate_path)?;
                        info!("Restoring state from {:?}...", savestate_path);
                        gba.restore_state(&save)?;
                        info!("Restored!");
                    } else {
                        info!("Savestate not created, please create one by pressing F5");
                    }
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
                    savestate_path = get_savestate_path(&Path::new(&rom_path));
                    rom_name = Path::new(&rom_path).file_name().unwrap().to_str().unwrap();
                    let gamepak = GamepakBuilder::new().file(Path::new(&rom_path)).build()?;
                    let bios_bin = read_bin_file(bios_path).unwrap();

                    // create a new emulator - TODO, export to a function
                    gba = GameBoyAdvance::new(
                        arm7tdmi::Core::new(),
                        bios_bin,
                        gamepak,
                        video.clone(),
                        audio.clone(),
                        input.clone(),
                    );
                    gba.skip_bios();
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

    Ok(())
}
