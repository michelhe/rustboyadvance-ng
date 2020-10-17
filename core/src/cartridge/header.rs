use serde::{Deserialize, Serialize};
use std::str::from_utf8;

use super::super::{GBAError, GBAResult};

/// From GBATEK
///
/// The first 192 bytes at 8000000h-80000BFh in ROM are used as cartridge header. The same header is also used for Multiboot images at 2000000h-20000BFh (plus some additional multiboot entries at 20000C0h and up).
///
/// Header Overview
///   Address Bytes Expl.
///   000h    4     ROM Entry Point  (32bit ARM branch opcode, eg. "B rom_start")
///   004h    156   Nintendo Logo    (compressed bitmap, required!)
///   0A0h    12    Game Title       (uppercase ascii, max 12 characters)
///   0ACh    4     Game Code        (uppercase ascii, 4 characters)
///   0B0h    2     Maker Code       (uppercase ascii, 2 characters)
///   0B2h    1     Fixed value      (must be 96h, required!)
///   0B3h    1     Main unit code   (00h for current GBA models)
///   0B4h    1     Device type      (usually 00h) (bit7=DACS/debug related)
///   0B5h    7     Reserved Area    (should be zero filled)
///   0BCh    1     Software version (usually 00h)
///   0BDh    1     Complement check (header checksum, required!)
///   0BEh    2     Reserved Area    (should be zero filled)
///   --- Additional Multiboot Header Entries ---
///   0C0h    4     RAM Entry Point  (32bit ARM branch opcode, eg. "B ram_start")
///   0C4h    1     Boot mode        (init as 00h - BIOS overwrites this value!)
///   0C5h    1     Slave ID Number  (init as 00h - BIOS overwrites this value!)
///   0C6h    26    Not used         (seems to be unused)
///   0E0h    4     JOYBUS Entry Pt. (32bit ARM branch opcode, eg. "B joy_start")
///
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CartridgeHeader {
    // rom_entry_point: Addr,
    pub game_title: String,
    pub game_code: String,
    pub maker_code: String,
    pub software_version: u8,
    pub checksum: u8,
    // ram_entry_point: Addr,
    // joybus_entry_point: Addr,
}

fn calculate_checksum(bytes: &[u8]) -> u8 {
    bytes
        .iter()
        .cloned()
        .fold(0u8, u8::wrapping_sub)
        .wrapping_sub(0x19)
}

pub fn parse(bytes: &[u8]) -> GBAResult<CartridgeHeader> {
    if bytes.len() < 0xc0 {
        return Err(GBAError::CartridgeLoadError(
            "incomplete cartridge header".to_string(),
        ));
    }

    let checksum = bytes[0xbd];
    let calculated_checksum = calculate_checksum(&bytes[0xa0..=0xbc]);
    if calculated_checksum != checksum {
        warn!(
            "invalid header checksum, calculated {:02x} but expected {:02x}",
            calculated_checksum, checksum
        );
    }

    let game_title = from_utf8(&bytes[0xa0..0xac])
        .map_err(|_| GBAError::CartridgeLoadError("invalid game title".to_string()))?;

    let game_code = from_utf8(&bytes[0xac..0xb0])
        .map_err(|_| GBAError::CartridgeLoadError("invalid game code".to_string()))?;

    let maker_code = from_utf8(&bytes[0xb0..0xb2])
        .map_err(|_| GBAError::CartridgeLoadError("invalid marker code".to_string()))?;

    // let (_, rom_entry_point) = le_u32(bytes).unwrap();
    // let (_, ram_entry_point) = le_u32(&bytes[0xc0..]).unwrap();
    // let (_, joybus_entry_point) = le_u32(&bytes[0xc0..]).unwrap();

    Ok(CartridgeHeader {
        // rom_entry_point: rom_entry_point,
        game_title: String::from(game_title),
        game_code: String::from(game_code),
        maker_code: String::from(maker_code),
        software_version: bytes[0xbc],
        checksum: checksum,
        // ram_entry_point: ram_entry_point,
        // joybus_entry_point: joybus_entry_point,
    })
}
