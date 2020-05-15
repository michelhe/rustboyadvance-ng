#[macro_use]
extern crate libretro_backend;

#[macro_use]
extern crate log;

use libretro_backend::{
    AudioVideoInfo, CoreInfo, GameData, JoypadButton, LoadGameResult, PixelFormat, RuntimeHandle,
};

use bit::BitIndex;
use unsafe_unwrap::UnsafeUnwrap;

use rustboyadvance_core::keypad::Keys as GbaButton;
use rustboyadvance_core::prelude::*;
use rustboyadvance_core::util::audio::AudioRingBuffer;

use std::path::Path;

use std::cell::RefCell;
use std::rc::Rc;

struct HwInterface {
    key_state: u16,
    audio_ring_buffer: AudioRingBuffer,
}

impl HwInterface {
    fn set_button_state(&mut self, button: JoypadButton, is_pressed: bool) {
        let mapped_button = match button {
            JoypadButton::A => GbaButton::ButtonA,
            JoypadButton::B => GbaButton::ButtonB,
            JoypadButton::Start => GbaButton::Start,
            JoypadButton::Select => GbaButton::Select,
            JoypadButton::Left => GbaButton::Left,
            JoypadButton::Up => GbaButton::Up,
            JoypadButton::Right => GbaButton::Right,
            JoypadButton::Down => GbaButton::Down,
            JoypadButton::L1 => GbaButton::ButtonL,
            JoypadButton::R1 => GbaButton::ButtonR,
            _ => unreachable!(),
        };
        self.key_state.set_bit(mapped_button as usize, !is_pressed);
    }
}

// do nothing here, everything is handled in the libretro_backend::Core impl
impl VideoInterface for HwInterface {}

impl AudioInterface for HwInterface {
    fn push_sample(&mut self, samples: StereoSample<i16>) {
        let prod = self.audio_ring_buffer.producer();
        prod.push(samples.0).unwrap();
        prod.push(samples.1).unwrap();
    }
}

impl InputInterface for HwInterface {
    fn poll(&mut self) -> u16 {
        self.key_state
    }
}

#[derive(Default)]
struct RustBoyAdvanceCore {
    gba: Option<GameBoyAdvance>,
    game_data: Option<GameData>,
    hwif: Option<Rc<RefCell<HwInterface>>>,
}

impl libretro_backend::Core for RustBoyAdvanceCore {
    fn info() -> CoreInfo {
        info!("Getting core info!");
        CoreInfo::new("RustBoyAdvance", env!("CARGO_PKG_VERSION"))
            .supports_roms_with_extension("gba")
    }

    fn on_load_game(&mut self, game_data: GameData) -> LoadGameResult {
        debug!("on_load_game");

        let system_directory = libretro_backend::environment::get_system_directory();
        if system_directory.is_none() {
            error!("no system directory!");
            return LoadGameResult::Failed(game_data);
        }
        let system_directory = system_directory.unwrap();
        let system_directory_path = Path::new(&system_directory);

        let bios_path = system_directory_path.join("gba_bios.bin");
        if !bios_path.exists() {
            error!("bios file missing, please place it in {:?}", bios_path);
            return LoadGameResult::Failed(game_data);
        }
        let bios = read_bin_file(&bios_path);

        if game_data.is_empty() {
            error!("game data is empty!");
            return LoadGameResult::Failed(game_data);
        }

        let result = if let Some(data) = game_data.data() {
            GamepakBuilder::new()
                .buffer(data)
                .without_backup_to_file()
                .build()
        } else if let Some(path) = game_data.path() {
            GamepakBuilder::new().file(Path::new(&path)).build()
        } else {
            unreachable!()
        };

        match (result, bios) {
            (Ok(gamepak), Ok(bios)) => {
                let av_info = AudioVideoInfo::new()
                    .video(240, 160, 60.0, PixelFormat::ARGB8888)
                    .audio(44100.0);

                let hwif = Rc::new(RefCell::new(HwInterface {
                    key_state: rustboyadvance_core::keypad::KEYINPUT_ALL_RELEASED,
                    audio_ring_buffer: AudioRingBuffer::new(),
                }));
                let gba = GameBoyAdvance::new(
                    bios.into_boxed_slice(),
                    gamepak,
                    hwif.clone(),
                    hwif.clone(),
                    hwif.clone(),
                );

                self.hwif = Some(hwif);
                self.gba = Some(gba);
                self.game_data = Some(game_data);
                LoadGameResult::Success(av_info)
            }
            _ => LoadGameResult::Failed(game_data),
        }
    }

    fn on_run(&mut self, handle: &mut RuntimeHandle) {
        let joypad_port = 0;

        // gba and hwif are `Some` after the game is loaded, so avoiding overhead of unwrap
        let gba = unsafe { self.gba.as_mut().unsafe_unwrap() };
        let hwif = unsafe { self.hwif.as_mut().unsafe_unwrap() };

        macro_rules! update_controllers {
            ( $( $button:ident ),+ ) => (
                $(
                    hwif.borrow_mut().set_button_state( JoypadButton::$button, handle.is_joypad_button_pressed( joypad_port, JoypadButton::$button ) );
                )+
            )
        }

        update_controllers!(A, B, Start, Select, Left, Up, Right, Down, L1, R1);

        gba.frame();

        let framebuffer = gba.get_frame_buffer();
        let bytes_per_pixel = 4;
        let framebuffer_size = 240 * 160;
        let uploaded_frame = unsafe {
            std::slice::from_raw_parts(
                framebuffer.as_ptr() as *const u8,
                framebuffer_size * bytes_per_pixel,
            )
        };
        handle.upload_video_frame(uploaded_frame);

        // upload sound samples
        {
            let mut audio_samples = [0; 4096 * 2];
            let mut hwif = hwif.borrow_mut();
            let consumer = hwif.audio_ring_buffer.consumer();
            let count = consumer.pop_slice(&mut audio_samples);

            handle.upload_audio_frame(&audio_samples[..count]);
        }
    }

    fn on_reset(&mut self) {
        debug!("on_reset");
        self.gba.as_mut().unwrap().soft_reset();
    }

    fn on_unload_game(&mut self) -> GameData {
        debug!("on_unload_game");
        self.game_data.take().unwrap()
    }
    // ...
}

libretro_core!(RustBoyAdvanceCore);
