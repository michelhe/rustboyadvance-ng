use sdl2::controller::Button;
use sdl2::event::Event;
use sdl2::keyboard::Scancode;
use sdl2::{self};

use bytesize;
use spin_sleep;
use structopt::StructOpt;

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time;

#[macro_use]
extern crate log;
use flexi_logger;
use flexi_logger::*;

mod audio;
mod input;
mod options;
mod video;

use rustboyadvance_core::prelude::*;

use rustboyadvance_utils::FpsCounter;

const LOG_DIR: &str = ".logs";

fn ask_download_bios() {
    const OPEN_SOURCE_BIOS_URL: &'static str =
        "https://github.com/Nebuleon/ReGBA/raw/master/bios/gba_bios.bin";
    println!("Missing BIOS file. If you don't have the original GBA BIOS, you can download an open-source bios from {}", OPEN_SOURCE_BIOS_URL);
    std::process::exit(0);
}

fn load_bios(bios_path: &Path) -> Box<[u8]> {
    match read_bin_file(bios_path) {
        Ok(bios) => bios.into_boxed_slice(),
        _ => {
            ask_download_bios();
            unreachable!()
        }
    }
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

    let opts = options::Options::from_args();

    info!("Initializing SDL2 context");
    let sdl_context = sdl2::init().expect("failed to initialize sdl2");

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

    let mut renderer = video::init(&sdl_context)?;
    let (audio_interface, mut _sdl_audio_device) = audio::create_audio_player(&sdl_context)?;
    let mut rom_name = opts.rom_name();

    let bios_bin = load_bios(&opts.bios);

    let mut gba = Box::new(GameBoyAdvance::new(
        bios_bin.clone(),
        opts.cartridge_from_opts()?,
        audio_interface,
    ));

    // let gba_raw_ptr = Box::into_raw(gba) as usize;
    // static mut gba_raw: usize = 0;
    // unsafe { gba_raw = gba_raw_ptr };
    // let mut gba = unsafe {Box::from_raw(gba_raw_ptr as *mut GameBoyAdvance) };

    // std::panic::set_hook(Box::new(|panic_info| {
    //     let gba = unsafe {Box::from_raw(gba_raw as *mut GameBoyAdvance) };
    //     println!("System crashed Oh No!!! {:?}", gba.cpu);
    //     let normal_panic = std::panic::take_hook();
    //     normal_panic(panic_info);
    // }));

    if opts.skip_bios {
        println!("Skipping bios animation..");
        gba.skip_bios();
    }

    if opts.gdbserver {
        gba.start_gdbserver(opts.gdbserver_port);
    }

    let mut vsync = true;
    let mut fps_counter = FpsCounter::default();
    const FRAME_TIME: time::Duration = time::Duration::new(0, 1_000_000_000u32 / 60);
    let mut event_pump = sdl_context.event_pump()?;
    'running: loop {
        let start_time = time::Instant::now();
        for event in event_pump.poll_iter() {
            match event {
                Event::KeyDown {
                    scancode: Some(scancode),
                    ..
                } => match scancode {
                    Scancode::Space => vsync = false,
                    k => input::on_keyboard_key_down(gba.get_key_state_mut(), k),
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
                    Scancode::F2 => gba.start_gdbserver(opts.gdbserver_port),
                    Scancode::F5 => {
                        info!("Saving state ...");
                        let save = gba.save_state()?;
                        write_bin_file(&opts.savestate_path(), &save)?;
                        info!(
                            "Saved to {:?} ({})",
                            opts.savestate_path(),
                            bytesize::ByteSize::b(save.len() as u64)
                        );
                    }
                    Scancode::F9 => {
                        if opts.savestate_path().is_file() {
                            let save = read_bin_file(&opts.savestate_path())?;
                            info!("Restoring state from {:?}...", opts.savestate_path());
                            let (audio_interface, _sdl_audio_device_new) =
                                audio::create_audio_player(&sdl_context)?;
                            _sdl_audio_device = _sdl_audio_device_new;
                            let rom = opts.read_rom()?.into_boxed_slice();
                            gba = Box::new(GameBoyAdvance::from_saved_state(
                                &save,
                                bios_bin.clone(),
                                rom,
                                audio_interface,
                            )?);
                            info!("Restored!");
                        } else {
                            info!("Savestate not created, please create one by pressing F5");
                        }
                    }
                    Scancode::Space => vsync = true,
                    k => input::on_keyboard_key_up(gba.get_key_state_mut(), k),
                },
                Event::ControllerButtonDown { button, .. } => match button {
                    Button::RightStick => vsync = !vsync,
                    b => input::on_controller_button_down(gba.get_key_state_mut(), b),
                },
                Event::ControllerButtonUp { button, .. } => {
                    input::on_controller_button_up(gba.get_key_state_mut(), button);
                }
                Event::ControllerAxisMotion { axis, value, .. } => {
                    input::on_axis_motion(gba.get_key_state_mut(), axis, value);
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
                    todo!("impl DropFile again")
                }
                _ => {}
            }
        }

        if gba.is_debugger_attached() {
            gba.debugger_run()
        } else {
            gba.frame();
        }
        renderer.render(gba.get_frame_buffer());

        if let Some(fps) = fps_counter.tick() {
            let title = format!("{} ({} fps)", rom_name, fps);
            renderer.set_window_title(&title);
        }

        if vsync {
            let time_passed = start_time.elapsed();
            let delay = FRAME_TIME.checked_sub(time_passed);
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
