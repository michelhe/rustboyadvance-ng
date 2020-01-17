use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::str::from_utf8;

use serde::{Deserialize, Serialize};
use memmem::{Searcher, TwoWaySearcher};
use num::FromPrimitive;
use zip::ZipArchive;

use super::super::util::read_bin_file;
use super::{Addr, Bus, GBAResult};

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
    game_title: String,
    game_code: String,
    maker_code: String,
    software_version: u8,
    checksum: u8,
    // ram_entry_point: Addr,
    // joybus_entry_point: Addr,
}

impl CartridgeHeader {
    fn parse(bytes: &[u8]) -> CartridgeHeader {
        // let (_, rom_entry_point) = le_u32(bytes).unwrap();
        let game_title = from_utf8(&bytes[0xa0..0xac]).unwrap();
        let game_code = from_utf8(&bytes[0xac..0xb0]).unwrap();
        let maker_code = from_utf8(&bytes[0xb0..0xb2]).unwrap();
        // let (_, ram_entry_point) = le_u32(&bytes[0xc0..]).unwrap();
        // let (_, joybus_entry_point) = le_u32(&bytes[0xc0..]).unwrap();

        CartridgeHeader {
            // rom_entry_point: rom_entry_point,
            game_title: String::from(game_title),
            game_code: String::from(game_code),
            maker_code: String::from(maker_code),
            software_version: bytes[0xbc],
            checksum: bytes[0xbd],
            // ram_entry_point: ram_entry_point,
            // joybus_entry_point: joybus_entry_point,
        }
    }
}

#[derive(Debug, Primitive, Serialize, Deserialize, Clone)]
pub enum BackupType {
    Eeprom = 0,
    Sram = 1,
    Flash = 2,
    Flash512 = 3,
    Flash1M = 4,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cartridge {
    pub header: CartridgeHeader,
    bytes: Box<[u8]>,
    size: usize,
}

fn load_rom(path: &Path) -> GBAResult<Vec<u8>> {
    match path.extension() {
        Some(extension) => match extension.to_str() {
            Some("zip") => {
                let zipfile = File::open(path)?;
                let mut archive = ZipArchive::new(zipfile)?;
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    if file.name().ends_with(".gba") {
                        let mut buf = Vec::new();
                        file.read_to_end(&mut buf)?;
                        return Ok(buf);
                    }
                }
                panic!("no .gba file contained in the zip file");
            }
            _ => {
                let buf = read_bin_file(path)?;
                return Ok(buf);
            }
        },
        _ => {
            let buf = read_bin_file(path)?;
            return Ok(buf);
        }
    }
}

impl Cartridge {
    pub fn from_path(rom_path: &Path) -> GBAResult<Cartridge> {
        let rom_bin = load_rom(rom_path)?;
        Ok(Cartridge::from_bytes(&rom_bin))
    }

    pub fn from_bytes(bytes: &[u8]) -> Cartridge {
        let backup = Cartridge::detect_backup_type(&bytes);
        println!("Backup detected: {:?}", backup);

        let size = bytes.len();
        let header = CartridgeHeader::parse(&bytes);

        Cartridge {
            header: header,
            bytes: bytes.into(),
            size: size,
        }
    }

    fn detect_backup_type(bin: &[u8]) -> Option<BackupType> {
        const ID_STRINGS: &'static [&'static str] =
            &["EEPROM_V", "SRAM_V", "FLASH_V", "FLASH512_V", "FLASH1M_V"];

        for i in 0..5 {
            let search = TwoWaySearcher::new(ID_STRINGS[i].as_bytes());
            match search.search_in(bin) {
                Some(_) => return Some(BackupType::from_u8(i as u8).unwrap()),
                _ => {}
            }
        }
        return None;
    }
}

impl Bus for Cartridge {
    fn read_8(&self, addr: Addr) -> u8 {
        if addr >= (self.size as u32) {
            0xDD // TODO - open bus implementation
        } else {
            self.bytes[addr as usize]
        }
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        self.bytes[addr as usize] = value;
    }
}
