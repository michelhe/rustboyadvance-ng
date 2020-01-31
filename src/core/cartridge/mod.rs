use serde::{Deserialize, Serialize};

use super::{Addr, Bus};

mod header;
use header::CartridgeHeader;

mod backup;
use backup::eeprom::EepromController;
use backup::flash::Flash;
pub use backup::BackupType;
use backup::{BackupFile, BackupMemoryInterface};

mod builder;
pub use builder::GamepakBuilder;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BackupMedia {
    Sram(BackupFile),
    Flash(Flash),
    Eeprom(EepromController),
    Undetected,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cartridge {
    pub header: CartridgeHeader,
    bytes: Box<[u8]>,
    size: usize,
    pub(in crate) backup: BackupMedia,
}

use super::sysbus::consts::*;

pub const EEPROM_BASE_ADDR: u32 = 0x0DFF_FF00;

impl Bus for Cartridge {
    fn read_8(&self, addr: Addr) -> u8 {
        let offset = (addr & 0x01ff_ffff) as usize;
        match addr & 0xff000000 {
            SRAM_LO | SRAM_HI => match &self.backup {
                BackupMedia::Sram(memory) => memory.read((addr & 0x7FFF) as usize),
                BackupMedia::Flash(flash) => flash.read(addr),
                _ => 0,
            },
            _ => {
                if offset >= self.size {
                    0xDD // TODO - open bus implementation
                } else {
                    self.bytes[offset as usize]
                }
            }
        }
    }

    fn read_16(&self, addr: u32) -> u16 {
        if addr & 0xff000000 == GAMEPAK_WS2_HI
            && (self.bytes.len() <= 16 * 1024 * 1024 || addr >= EEPROM_BASE_ADDR)
        {
            if let BackupMedia::Eeprom(spi) = &self.backup {
                return spi.read_half(addr);
            }
        }
        self.default_read_16(addr)
    }

    fn write_8(&mut self, addr: u32, value: u8) {
        match addr & 0xff000000 {
            SRAM_LO | SRAM_HI => match &mut self.backup {
                BackupMedia::Flash(flash) => flash.write(addr, value),
                BackupMedia::Sram(memory) => memory.write((addr & 0x7FFF) as usize, value),
                _ => {}
            },
            _ => {} // TODO allow the debugger to write
        };
    }

    fn write_16(&mut self, addr: u32, value: u16) {
        if addr & 0xff000000 == GAMEPAK_WS2_HI
            && (self.bytes.len() <= 16 * 1024 * 1024 || addr >= EEPROM_BASE_ADDR)
        {
            if let BackupMedia::Eeprom(spi) = &mut self.backup {
                return spi.write_half(addr, value);
            }
        }
        self.default_write_16(addr, value);
    }
}
