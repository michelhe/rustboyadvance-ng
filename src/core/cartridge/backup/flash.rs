use super::{BackupMemory, BackupMemoryInterface};

use num::FromPrimitive;
use serde::{Deserialize, Serialize};

use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
enum FlashWriteSequence {
    Initial,
    Magic,
    Command,
    Argument,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum FlashMode {
    Initial,
    ChipId,
    Erase,
    Write,
    Select,
}

#[derive(Debug, Primitive)]
enum FlashCommand {
    EnterIdMode = 0x90,
    TerminateIdMode = 0xf0,
    Erase = 0x80,
    EraseEntireChip = 0x10,
    EraseSector = 0x30,
    WriteByte = 0xa0,
    SelectBank = 0xb0,
}

#[derive(Debug)]
pub enum FlashSize {
    Flash64k,
    Flash128k,
}

impl Into<usize> for FlashSize {
    fn into(self) -> usize {
        match self {
            FlashSize::Flash64k => 64 * 1024,
            FlashSize::Flash128k => 128 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flash {
    chip_id: u16,
    size: usize,
    wrseq: FlashWriteSequence,
    mode: FlashMode,
    bank: usize,

    memory: BackupMemory,
}

const MACRONIX_64K_CHIP_ID: u16 = 0x1CC2;
const MACRONIX_128K_CHIP_ID: u16 = 0x09c2;

const SECTOR_SIZE: usize = 0x1000;
const BANK_SIZE: usize = 0x10000;

impl Flash {
    pub fn new(flash_path: PathBuf, flash_size: FlashSize) -> Flash {
        let chip_id = match flash_size {
            FlashSize::Flash64k => MACRONIX_64K_CHIP_ID,
            FlashSize::Flash128k => MACRONIX_128K_CHIP_ID,
        };

        let size: usize = flash_size.into();
        let memory = BackupMemory::new(size, flash_path);

        Flash {
            chip_id: chip_id,
            wrseq: FlashWriteSequence::Initial,
            mode: FlashMode::Initial,
            size: size,
            bank: 0,
            memory: memory,
        }
    }

    fn reset_sequence(&mut self) {
        self.wrseq = FlashWriteSequence::Initial;
    }

    fn command(&mut self, addr: u32, value: u8) {
        const COMMAND_ADDR: u32 = 0x0E00_5555;
        if let Some(command) = FlashCommand::from_u8(value) {
            match (addr, command) {
                (COMMAND_ADDR, FlashCommand::EnterIdMode) => {
                    self.mode = FlashMode::ChipId;
                    self.reset_sequence();
                }
                (COMMAND_ADDR, FlashCommand::TerminateIdMode) => {
                    self.mode = FlashMode::Initial;
                    self.reset_sequence();
                }
                (COMMAND_ADDR, FlashCommand::Erase) => {
                    self.mode = FlashMode::Erase;
                    self.reset_sequence();
                }
                (COMMAND_ADDR, FlashCommand::EraseEntireChip) => {
                    if self.mode == FlashMode::Erase {
                        for i in 0..self.size {
                            self.memory.write(i, 0xff);
                        }
                    }
                    self.reset_sequence();
                    self.mode = FlashMode::Initial;
                }
                (sector_n, FlashCommand::EraseSector) => {
                    let sector_offset = self.flash_offset((sector_n & 0xf000) as usize);

                    for i in 0..SECTOR_SIZE {
                        self.memory.write(sector_offset + i, 0xff);
                    }
                    self.reset_sequence();
                    self.mode = FlashMode::Initial;
                }
                (COMMAND_ADDR, FlashCommand::WriteByte) => {
                    self.mode = FlashMode::Write;
                    self.wrseq = FlashWriteSequence::Argument;
                }
                (COMMAND_ADDR, FlashCommand::SelectBank) => {
                    self.mode = FlashMode::Select;
                    self.wrseq = FlashWriteSequence::Argument;
                }
                (addr, command) => {
                    panic!("[FLASH] Invalid command {:?} addr {:#x}", command, addr);
                }
            };
        } else {
            panic!("[FLASH] unknown command {:x}", value);
        }
    }

    /// Returns the phyiscal offset inside the flash file according to the selected bank
    #[inline]
    fn flash_offset(&self, offset: usize) -> usize {
        let offset = (offset & 0xffff) as usize;
        return self.bank * BANK_SIZE + offset;
    }

    pub fn read(&self, addr: u32) -> u8 {
        let offset = (addr & 0xffff) as usize;
        let result = if self.mode == FlashMode::ChipId {
            match offset {
                0 => (self.chip_id & 0xff) as u8,
                1 => (self.chip_id >> 8) as u8,
                _ => panic!("Tried to read invalid flash offset while reading chip ID"),
            }
        } else {
            self.memory.read(self.flash_offset(offset))
        };

        result
    }

    pub fn write(&mut self, addr: u32, value: u8) {
        // println!("[FLASH] write {:#x}={:#x}", addr, value);
        match self.wrseq {
            FlashWriteSequence::Initial => {
                if addr == 0x0E00_5555 && value == 0xAA {
                    self.wrseq = FlashWriteSequence::Magic;
                }
            }
            FlashWriteSequence::Magic => {
                if addr == 0xE00_2AAA && value == 0x55 {
                    self.wrseq = FlashWriteSequence::Command;
                }
            }
            FlashWriteSequence::Command => {
                self.command(addr, value);
            }
            FlashWriteSequence::Argument => {
                match self.mode {
                    FlashMode::Write => {
                        self.memory
                            .write(self.flash_offset((addr & 0xffff) as usize), value);
                    }
                    FlashMode::Select => {
                        if addr == 0x0E00_0000 {
                            self.bank = value as usize;
                        }
                    }
                    _ => panic!("Flash sequence is invalid"),
                };
                self.mode = FlashMode::Initial;
                self.reset_sequence();
            }
        }
    }
}
