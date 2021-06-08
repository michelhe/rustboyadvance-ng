use sdl2;
use sdl2::controller::Button;
use sdl2::event::{Event, WindowEvent};
use sdl2::image::{InitFlag, LoadSurface, LoadTexture};
use sdl2::keyboard::Scancode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::WindowCanvas;
use sdl2::surface::Surface;

use sdl2::EventPump;

use bytesize;
use spin_sleep;

use std::cell::RefCell;
use std::rc::Rc;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process;
use std::time;

use std::convert::TryFrom;

#[macro_use]
extern crate clap;

#[macro_use]
extern crate log;
use flexi_logger;
use flexi_logger::*;

mod audio;
mod input;
mod video;

use audio::{create_audio_player, create_dummy_player};
use input::create_input;
use video::{create_video_interface, SCREEN_HEIGHT, SCREEN_WIDTH};

use rustboyadvance_core::cartridge::BackupType;
use rustboyadvance_core::prelude::*;
use rustboyadvance_core::util::spawn_and_run_gdb_server;
use rustboyadvance_core::util::FpsCounter;

const LOG_DIR: &str = ".logs";
const DEFAULT_GDB_SERVER_ADDR: &'static str = "localhost:1337";

const CANVAS_WIDTH: u32 = SCREEN_WIDTH;
const CANVAS_HEIGHT: u32 = SCREEN_HEIGHT;

fn get_savestate_path(rom_filename: &Path) -> PathBuf {
    rom_filename.with_extension("savestate")
}

/// Waits for the user to drag a rom file to window
fn wait_for_rom(canvas: &mut WindowCanvas, event_pump: &mut EventPump) -> Result<String, String> {
    let texture_creator = canvas.texture_creator();
    let icon_texture = texture_creator
        .load_texture("assets/icon_cropped_small.png")
        .expect("failed to load icon");
    let background = Color::RGB(0xDD, 0xDD, 0xDD);

    let mut redraw = || -> Result<(), String> {
        canvas.set_draw_color(background);
        canvas.clear();
        canvas.copy(
            &icon_texture,
            None,
            Some(Rect::from_center(
                ((CANVAS_WIDTH / 2) as i32, (CANVAS_HEIGHT / 2) as i32),
                160,
                100,
            )),
        )?;
        canvas.present();
        Ok(())
    };

    redraw()?;

    loop {
        for event in event_pump.wait_iter() {
            match event {
                Event::DropFile { filename, .. } => {
                    return Ok(filename);
                }
                Event::Quit { .. } => process::exit(0),
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::SizeChanged(..) | WindowEvent::Restored => redraw()?,
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn ask_download_bios() {
    const OPEN_SOURCE_BIOS_URL: &'static str =
        "https://github.com/Nebuleon/ReGBA/raw/master/bios/gba_bios.bin";
    println!("Missing BIOS file. If you don't have the original GBA BIOS, you can download an open-source bios from {}", OPEN_SOURCE_BIOS_URL);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(LOG_DIR).expect(&format!("could not create log directory ({})", LOG_DIR));
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

    let bios_path = Path::new(matches.value_of("bios").unwrap_or_default());
    let bios_bin = match read_bin_file(bios_path) {
        Ok(bios) => bios.into_boxed_slice(),
        _ => {
            ask_download_bios();
            std::process::exit(0);
        }
    };

    let skip_bios = matches.occurrences_of("skip_bios") != 0;

    let debug = matches.occurrences_of("debug") != 0;
    let silent = matches.occurrences_of("silent") != 0;
    let with_gdbserver = matches.occurrences_of("with_gdbserver") != 0;

    info!("Initializing SDL2 context");
    let sdl_context = sdl2::init().expect("failed to initialize sdl2");

    let mut event_pump = sdl_context.event_pump()?;

    let video_subsystem = sdl_context.video()?;
    let _image_context = sdl2::image::init(InitFlag::PNG | InitFlag::JPG)?;
    let mut window = video_subsystem
        .window("RustBoyAdvance", SCREEN_WIDTH * 3, SCREEN_HEIGHT * 3)
        .opengl()
        .position_centered()
        .resizable()
        .build()?;

    let window_icon = Surface::from_file("assets/icon.png")?;
    window.set_icon(window_icon);

    let mut canvas = window.into_canvas().accelerated().build()?;
    canvas.set_logical_size(CANVAS_WIDTH, CANVAS_HEIGHT)?;

    let controller_subsystem = sdl_context.game_controller()?;
    let controller_mappings =
        include_str!("../../../external/SDL_GameControllerDB/gamecontrollerdb.txt");
    controller_subsystem.load_mappings_from_read(&mut Cursor::new(controller_mappings))?;

    let available_controllers = (0..controller_subsystem.num_joysticks()?)
        .filter(|&id| controller_subsystem.is_game_controller(id))
        .collect::<Vec<u32>>();

    let mut active_controller = match available_controllers.first() {
        Some(&id) => {
            let controller = controller_subsystem.open(id)?;
            info!("Found game controller: {}", controller.name());
            Some(controller)
        }
        _ => {
            info!("No game controllers were found");
            None
        }
    };

    let mut rom_path = match matches.value_of("game_rom") {
        Some(path) => path.to_string(),
        _ => {
            info!("[!] Rom file missing, please drag a rom file into the emulator window...");
            wait_for_rom(&mut canvas, &mut event_pump)?
        }
    };

    let video = Rc::new(RefCell::new(create_video_interface(canvas)));
    let audio: Rc<RefCell<dyn AudioInterface>> = if silent {
        Rc::new(RefCell::new(create_dummy_player()))
    } else {
        Rc::new(RefCell::new(create_audio_player(&sdl_context)))
    };
    let input = Rc::new(RefCell::new(create_input()));

    let mut savestate_path = get_savestate_path(&Path::new(&rom_path));

    let mut rom_name = Path::new(&rom_path).file_name().unwrap().to_str().unwrap();

    let mut builder = GamepakBuilder::new()
        .save_type(BackupType::try_from(
            matches.value_of("save_type").unwrap(),
        )?)
        .file(Path::new(&rom_path));

    if matches.occurrences_of("rtc") != 0 {
        builder = builder.with_rtc();
    }

    let gamepak = builder.build()?;

    let mut gba = GameBoyAdvance::new(
        bios_bin.clone(),
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
            let mut debugger = Debugger::new();
            info!("starting debugger...");
            debugger
                .repl(&mut gba, matches.value_of("script_file"))
                .unwrap();
            info!("ending debugger...");
            return Ok(());
        }
        #[cfg(not(feature = "debugger"))]
        {
            panic!("Please compile me with 'debugger' feature");
        }
    }

    if with_gdbserver {
        spawn_and_run_gdb_server(&mut gba, DEFAULT_GDB_SERVER_ADDR)?;
    }

    let mut fps_counter = FpsCounter::default();
    let frame_time = time::Duration::new(0, 1_000_000_000u32 / 60);
    'running: loop {
        let start_time = time::Instant::now();

        for event in event_pump.poll_iter() {
            match event {
                Event::KeyDown {
                    scancode: Some(scancode),
                    ..
                } => match scancode {
                    Scancode::Space => frame_limiter = false,
                    k => input.borrow_mut().on_keyboard_key_down(k),
                },
                Event::KeyUp {
                    scancode: Some(scancode),
                    ..
                } => match scancode {
                    #[cfg(feature = "debugger")]
                    Scancode::F1 => {
                        let mut debugger = Debugger::new();
                        info!("starting debugger...");
                        debugger
                            .repl(&mut gba, matches.value_of("script_file"))
                            .unwrap();
                        info!("ending debugger...")
                    }
                    #[cfg(feature = "gdb")]
                    Scancode::F2 => spawn_and_run_gdb_server(&mut gba, DEFAULT_GDB_SERVER_ADDR)?,
                    Scancode::F5 => {
                        info!("Saving state ...");
                        let save = gba.save_state()?;
                        write_bin_file(&savestate_path, &save)?;
                        info!(
                            "Saved to {:?} ({})",
                            savestate_path,
                            bytesize::ByteSize::b(save.len() as u64)
                        );
                    }
                    Scancode::F9 => {
                        if savestate_path.is_file() {
                            let save = read_bin_file(&savestate_path)?;
                            info!("Restoring state from {:?}...", savestate_path);
                            gba.restore_state(&save)?;
                            info!("Restored!");
                        } else {
                            info!("Savestate not created, please create one by pressing F5");
                        }
                    }
                    Scancode::Space => frame_limiter = true,
                    k => input.borrow_mut().on_keyboard_key_up(k),
                },
                Event::ControllerButtonDown { button, .. } => match button {
                    Button::RightStick => frame_limiter = !frame_limiter,
                    b => input.borrow_mut().on_controller_button_down(b),
                },
                Event::ControllerButtonUp { button, .. } => {
                    input.borrow_mut().on_controller_button_up(button);
                }
                Event::ControllerAxisMotion { axis, value, .. } => {
                    input.borrow_mut().on_axis_motion(axis, value);
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    let removed = if let Some(active_controller) = &active_controller {
                        active_controller.instance_id() == (which as i32)
                    } else {
                        false
                    };
                    if removed {
                        let name = active_controller
                            .and_then(|controller| Some(controller.name()))
                            .unwrap();
                        info!("Removing game controller: {}", name);
                        active_controller = None;
                    }
                }
                Event::ControllerDeviceAdded { which, .. } => {
                    if active_controller.is_none() {
                        let controller = controller_subsystem.open(which)?;
                        info!("Adding game controller: {}", controller.name());
                        active_controller = Some(controller);
                    }
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
                        bios_bin.into_boxed_slice(),
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
