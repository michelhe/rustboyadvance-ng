use crossterm::event::EventStream;
use crossterm::ExecutableCommand;
use clap::Parser;

use std::path::Path;
use std::time;

mod audio;
mod input;
mod options;
mod video;

use rustboyadvance_core::prelude::*;

use std::io;
use tokio::sync::mpsc;

use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use futures::{FutureExt, StreamExt};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Error,
    Tick,
    Resize(u16, u16),
    Key(KeyEvent),
}

#[derive(Debug)]
pub struct EventHandler {
    _tx: mpsc::UnboundedSender<Event>,
    rx: mpsc::UnboundedReceiver<Event>,
    _task: Option<JoinHandle<()>>,
}

impl EventHandler {
    pub fn new() -> Self {
        const FRAME_TIME: time::Duration = time::Duration::new(0, 1_000_000_000u32 / 60);

        let (tx, rx) = mpsc::unbounded_channel();
        let _tx = tx.clone();

        let task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut interval = tokio::time::interval(FRAME_TIME);
            loop {
                let delay = interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                  maybe_event = crossterm_event => {
                    match maybe_event {
                      Some(Ok(evt)) => {
                        match evt {
                          crossterm::event::Event::Key(key) => {
                            if key.kind != crossterm::event::KeyEventKind::Repeat {
                              tx.send(Event::Key(key)).unwrap();
                            }
                          },
                          crossterm::event::Event::Resize(x, y) => {
                            let _ = tx.send(Event::Resize(x, y));
                          }
                          _ => {},
                        }
                      }
                      Some(Err(_)) => {
                        let _ = tx.send(Event::Error);
                      }
                      None => {},
                    }
                  },
                  _ = delay => {
                      let _ = tx.send(Event::Tick);
                  },
                }
            }
        });

        Self {
            _tx,
            rx,
            _task: Some(task),
        }
    }

    pub async fn next(&mut self) -> Result<Event> {
        self.rx
            .recv()
            .await
            .ok_or(color_eyre::eyre::eyre!("Unable to get event"))
    }
}

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

#[tokio::main]
async fn main() -> Result<()> {
    let opts = options::Options::parse();

    let (audio_interface, _output_device) = audio::create_audio_player();
    let _rom_name = opts.rom_name();

    let bios_bin = load_bios(&opts.bios);

    let mut gba = Box::new(GameBoyAdvance::new(
        bios_bin.clone(),
        opts.cartridge_from_opts().unwrap(),
        audio_interface,
    ));

    if opts.skip_bios {
        println!("Skipping bios animation..");
        gba.skip_bios();
    }

    if opts.gdbserver {
        gba.start_gdbserver(opts.gdbserver_port);
    }

    //let mut event_pump = sdl_context.event_pump()?;

    async fn run(
        opts: options::Options,
        mut gba: Box<GameBoyAdvance>,
        mut _output_device: tinyaudio::OutputDevice,
        bios_bin: Box<[u8]>,
    ) -> Result<()> {
        let mut stdout = io::stdout();

        //renderer must be loaded before event handler due to kitty support
        let mut renderer = video::init();

        let mut reader = EventHandler::new();

        let _ = crossterm::terminal::enable_raw_mode();

        let _r = crossterm::execute!(
            io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | crossterm::event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        );

        let _ = stdout.execute(crossterm::cursor::Hide);
        renderer.clear(&mut stdout);
        let mut information = None;

        'running: loop {
            let event = reader.next().await?;
            match event {
                Event::Error => return Ok(()),
                Event::Tick => {
                    if gba.is_debugger_attached() {
                        gba.debugger_run()
                    } else {
                        gba.frame();
                    }
                    renderer.render(gba.get_frame_buffer(), &information);
                    information = None;
                }
                Event::Resize(_, _) => {
                    renderer.clear(&mut stdout);
                }
                Event::Key(key_event) => {
                    match key_event.kind {
                        crossterm::event::KeyEventKind::Press => match key_event.code {
                            crossterm::event::KeyCode::Char('c') => {
                                if key_event
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::CONTROL)
                                {
                                    break 'running;
                                }
                            }
                            c => input::on_keyboard_key_down(gba.get_key_state_mut(), c),
                        },
                        crossterm::event::KeyEventKind::Release => match key_event.code {
                            #[cfg(feature = "debugger")]
                            crossterm::event::KeyCode::F(1) => {
                                let mut debugger = Debugger::new();
                                debugger
                                    .repl(&mut gba, opts.script_file.as_deref())
                                    .unwrap();
                            }
                            crossterm::event::KeyCode::F(2) => {
                                gba.start_gdbserver(opts.gdbserver_port)
                            }
                            crossterm::event::KeyCode::F(5) => {
                                let save = gba.save_state()?;
                                write_bin_file(&opts.savestate_path(), &save)?;
                                information = Some(format!("{:?} saved", &opts.savestate_path()))
                            }
                            crossterm::event::KeyCode::F(9) => {
                                if opts.savestate_path().is_file() {
                                    let save = read_bin_file(&opts.savestate_path())?;
                                    let (audio_interface, _audio_device_new) =
                                        audio::create_audio_player();
                                    _output_device = _audio_device_new;
                                    let rom = opts.read_rom()?.into_boxed_slice();
                                    gba = Box::new(GameBoyAdvance::from_saved_state(
                                        &save,
                                        bios_bin.clone(),
                                        rom,
                                        audio_interface,
                                    )?);
                                    information =
                                        Some(format!("{:?} loaded", &opts.savestate_path()))
                                } else {
                                    information =
                                        Some(format!("failed to load {:?}", &opts.savestate_path()))
                                }
                            }
                            crossterm::event::KeyCode::Esc => break 'running,
                            c => input::on_keyboard_key_up(gba.get_key_state_mut(), c),
                        },
                        _ => {}
                    };
                }
            }
        }

        let _r = crossterm::execute!(io::stdout(), crossterm::event::PopKeyboardEnhancementFlags);
        Ok(())
    }

    let r = run(opts, gba, _output_device, bios_bin).await?;

    Ok(r)
}
