use std::str::from_utf8;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use super::arm7tdmi::{
    bus::{Bus, MemoryAccess, MemoryAccessWidth},
    Addr,
};
use super::sysbus::WaitState;
use crate::util::read_bin_file;

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
#[derive(Debug)]
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

#[derive(Debug)]
pub struct Cartridge {
    pub header: CartridgeHeader,
    bytes: Box<[u8]>,
    ws: WaitState,
}

impl Cartridge {
    const MIN_SIZE: usize = 4 * 1024 * 1024;

    pub fn load(path: &str) -> Result<Cartridge, ::std::io::Error> {
        let mut rom_bin = read_bin_file(path)?;
        if rom_bin.len() < Cartridge::MIN_SIZE {
            rom_bin.resize_with(Cartridge::MIN_SIZE, Default::default);
        }

        let header = CartridgeHeader::parse(&rom_bin);
        Ok(Cartridge {
            header: header,
            bytes: rom_bin.into_boxed_slice(),
            ws: WaitState::new(5, 5, 8),
        })
    }
}

impl Bus for Cartridge {
    fn read_32(&self, addr: Addr) -> u32 {
        (&self.bytes[addr as usize..])
            .read_u32::<LittleEndian>()
            .unwrap()
    }

    fn read_16(&self, addr: Addr) -> u16 {
        (&self.bytes[addr as usize..])
            .read_u16::<LittleEndian>()
            .unwrap()
    }

    fn read_8(&self, addr: Addr) -> u8 {
        (&self.bytes[addr as usize..])[0]
    }

    fn write_32(&mut self, addr: Addr, value: u32) {
        (&mut self.bytes[addr as usize..])
            .write_u32::<LittleEndian>(value)
            .unwrap()
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        (&mut self.bytes[addr as usize..])
            .write_u16::<LittleEndian>(value)
            .unwrap()
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        (&mut self.bytes[addr as usize..]).write_u8(value).unwrap()
    }

    fn get_cycles(&self, _addr: Addr, access: MemoryAccess) -> usize {
        match access.1 {
            MemoryAccessWidth::MemoryAccess8 => self.ws.access8,
            MemoryAccessWidth::MemoryAccess16 => self.ws.access16,
            MemoryAccessWidth::MemoryAccess32 => self.ws.access32,
        }
    }
}
