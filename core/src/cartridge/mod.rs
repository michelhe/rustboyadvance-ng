use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::bus::*;

pub mod header;
use header::CartridgeHeader;

mod backup;
use backup::eeprom::EepromController;
use backup::flash::Flash;
pub use backup::BackupType;
use backup::{BackupFile, BackupMemoryInterface};

mod gpio;
use gpio::*;
mod rtc;
use rtc::Rtc;

mod builder;
mod loader;
pub use builder::GamepakBuilder;

pub const GPIO_PORT_DATA: u32 = 0xC4;
pub const GPIO_PORT_DIRECTION: u32 = 0xC6;
pub const GPIO_PORT_CONTROL: u32 = 0xC8;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BackupMedia {
    Sram(BackupFile),
    Flash(Flash),
    Eeprom(EepromController),
    Undetected,
}

pub type SymbolTable = HashMap<String, u32>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cartridge {
    pub header: CartridgeHeader,
    #[serde(skip)]
    bytes: Box<[u8]>,
    #[serde(skip)]
    size: usize,
    symbols: Option<SymbolTable>, // TODO move it somewhere else
    pub(in crate) backup: BackupMedia,
    // Gpio stuff
    rtc: Option<Rtc>,
    gpio_direction: [GpioDirection; 4],
    gpio_control: GpioPortControl,
}

impl Cartridge {
    pub fn get_symbols(&self) -> &Option<SymbolTable> {
        &self.symbols
    }
    pub fn get_rtc(&self) -> &Option<Rtc> {
        &self.rtc
    }

    pub fn get_rtc_mut(&mut self) -> &mut Option<Rtc> {
        &mut self.rtc
    }

    pub fn set_rom_bytes(&mut self, bytes: Box<[u8]>) {
        self.size = bytes.len();
        self.bytes = bytes;
    }

    pub fn get_rom_bytes(&self) -> &[u8] {
        &self.bytes
    }

    // 'Clones' the cartridge without the ROM buffer
    pub fn thin_copy(&self) -> Cartridge {
        Cartridge {
            header: self.header.clone(),
            bytes: Default::default(),
            size: 0,
            symbols: self.symbols.clone(),
            backup: self.backup.clone(),
            rtc: self.rtc.clone(),
            gpio_control: self.gpio_control,
            gpio_direction: self.gpio_direction,
        }
    }

    pub fn update_from(&mut self, other: Cartridge) {
        self.header = other.header;
        self.rtc = other.rtc;
        self.gpio_control = other.gpio_control;
        self.gpio_direction = other.gpio_direction;
        self.symbols = other.symbols;
        self.backup = other.backup;
    }

    #[inline]
    /// From GBATEK:
    /// Reading from GamePak ROM when no Cartridge is inserted -
    ///     Because Gamepak uses the same signal-lines for both 16bit data and for lower 16bit halfword address,
    ///     the entire gamepak ROM area is effectively filled by incrementing 16bit values (Address/2 AND FFFFh).
    fn read_unused(&self, addr: Addr) -> u8 {
        let x = (addr / 2) & 0xffff;
        if addr & 1 != 0 {
            (x >> 8) as u8
        } else {
            x as u8
        }
    }
}

use super::sysbus::consts::*;

pub const EEPROM_BASE_ADDR: u32 = 0x0DFF_FF00;

fn is_gpio_access(addr: u32) -> bool {
    match addr & 0x1ff_ffff {
        GPIO_PORT_DATA | GPIO_PORT_DIRECTION | GPIO_PORT_CONTROL => true,
        _ => false,
    }
}

impl Bus for Cartridge {
    fn read_8(&mut self, addr: Addr) -> u8 {
        let offset = (addr & 0x01ff_ffff) as usize;
        match addr & 0xff000000 {
            SRAM_LO | SRAM_HI => match &self.backup {
                BackupMedia::Sram(memory) => memory.read((addr & 0x7FFF) as usize),
                BackupMedia::Flash(flash) => flash.read(addr),
                _ => 0,
            },
            _ => {
                if offset >= self.size {
                    self.read_unused(addr)
                } else {
                    unsafe { *self.bytes.get_unchecked(offset as usize) }
                }
            }
        }
    }

    fn read_16(&mut self, addr: u32) -> u16 {
        if is_gpio_access(addr) {
            if self.rtc.is_some() {
                if !(self.is_gpio_readable()) {
                    warn!("trying to read GPIO when reads are not allowed");
                }
                return self.gpio_read(addr & 0x1ff_ffff);
            }
        }

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
        if is_gpio_access(addr) {
            if self.rtc.is_some() {
                self.gpio_write(addr & 0x1ff_ffff, value);
                return;
            }
        }

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

impl DebugRead for Cartridge {
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        let offset = (addr & 0x01ff_ffff) as usize;
        if offset >= self.size {
            self.read_unused(addr)
        } else {
            self.bytes[offset]
        }
    }
}
