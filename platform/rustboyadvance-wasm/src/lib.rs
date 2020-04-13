mod utils;

use wasm_bindgen::prelude::*;

#[macro_use]
extern crate log;

use wasm_bindgen_console_logger::DEFAULT_LOGGER;

use rustboyadvance_core::core::cartridge;

pub mod emulator;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
pub fn init() {
    utils::set_panic_hook();

    log::set_logger(&DEFAULT_LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    info!("Initialized wasm module");
}

#[wasm_bindgen]
pub struct RomInfo {
    game_code: String,
    game_title: String,
}

#[wasm_bindgen]
impl RomInfo {
    pub fn get_game_code(&self) -> String {
        self.game_code.to_string()
    }

    pub fn get_game_title(&self) -> String {
        self.game_title.to_string()
    }
}

impl From<cartridge::header::CartridgeHeader> for RomInfo {
    fn from(header: cartridge::header::CartridgeHeader) -> RomInfo {
        RomInfo {
            game_code: header.game_code,
            game_title: header.game_title,
        }
    }
}

#[wasm_bindgen]
pub fn parse_rom_header(rom_bin: &[u8]) -> RomInfo {
    cartridge::header::parse(rom_bin).into()
}
