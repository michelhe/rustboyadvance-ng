#[macro_use]
extern crate libretro_backend;

#[macro_use]
extern crate log;

use libretro_backend::{
    AudioVideoInfo, CoreInfo, GameData, JoypadButton, LoadGameResult, PixelFormat, RuntimeHandle,
};

use bit::BitIndex;
use unsafe_unwrap::UnsafeUnwrap;

use rustboyadvance_core::keypad::Keys as _GbaButton;
use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::audio::SampleConsumer;

use std::ops::Deref;
use std::path::Path;

use std::default::Default;

#[derive(Default)]
struct RustBoyAdvanceCore {
    gba: Option<GameBoyAdvance>,
    game_data: Option<GameData>,
    audio_consumer: Option<SampleConsumer>,
}

#[repr(transparent)]
struct GbaButton(_GbaButton);

impl Deref for GbaButton {
    type Target = _GbaButton;
    fn deref(&self) -> &Self::Target {
        return &self.0;
    }
}

impl From<JoypadButton> for GbaButton {
    fn from(button: JoypadButton) -> Self {
        let mapped = match button {
            JoypadButton::A => _GbaButton::ButtonA,
            JoypadButton::B => _GbaButton::ButtonB,
            JoypadButton::Start => _GbaButton::Start,
            JoypadButton::Select => _GbaButton::Select,
            JoypadButton::Left => _GbaButton::Left,
            JoypadButton::Up => _GbaButton::Up,
            JoypadButton::Right => _GbaButton::Right,
            JoypadButton::Down => _GbaButton::Down,
            JoypadButton::L1 => _GbaButton::ButtonL,
            JoypadButton::R1 => _GbaButton::ButtonR,
            _ => panic!("unimplemented button {:?}", button),
        };
        GbaButton(mapped)
    }
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

                let (audio_device, audio_consumer) =
                    SimpleAudioInterface::create_channel(44100, None);

                let gba = GameBoyAdvance::new(bios.into_boxed_slice(), gamepak, audio_device);

                self.audio_consumer = Some(audio_consumer);
                self.gba = Some(gba);
                self.game_data = Some(game_data);
                LoadGameResult::Success(av_info)
            }
            _ => LoadGameResult::Failed(game_data),
        }
    }

    fn on_run(&mut self, handle: &mut RuntimeHandle) {
        let joypad_port = 0;

        // gba and audio are `Some` after the game is loaded, so avoiding overhead of unwrap
        let gba = unsafe { self.gba.as_mut().unsafe_unwrap() };

        let key_state = gba.get_key_state_mut();
        macro_rules! update_controllers {
            ( $( $button:ident ),+ ) => (
                $(
                    key_state.set_bit(*GbaButton::from(JoypadButton::$button) as usize, !handle.is_joypad_button_pressed( joypad_port, JoypadButton::$button ));
                )+
            )
        }

        update_controllers!(A, B, Start, Select, Left, Up, Right, Down, L1, R1);
        drop(key_state);

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
            let mut audio_consumer = self.audio_consumer.take().unwrap();
            let mut audio_samples = [0; 4096 * 2];
            let count = audio_consumer.pop_slice(&mut audio_samples);
            handle.upload_audio_frame(&audio_samples[..count]);
            self.audio_consumer.replace(audio_consumer);
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
