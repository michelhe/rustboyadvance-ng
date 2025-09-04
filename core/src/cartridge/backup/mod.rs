use std::fmt;

use clap::ValueEnum;

mod backup_file;
pub use backup_file::BackupFile;
use num_derive::{FromPrimitive, ToPrimitive};
pub mod eeprom;
pub mod flash;

#[derive(Debug, ToPrimitive, FromPrimitive, Serialize, Deserialize, Copy, Clone, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum BackupType {
    Eeprom = 0,
    Sram = 1,
    Flash = 2,
    Flash512 = 3,
    Flash1M = 4,
    AutoDetect = 5,
}

impl From<String> for BackupType {  
    fn from(value: String) -> Self {
        use BackupType::*;
        match value.as_str() {
            "autodetect" => AutoDetect,
            "sram" => Sram,
            "flash128k" => Flash1M,
            "flash64k" => Flash512,
            "eeprom" => Eeprom,
            _ => panic!("invalid save type {}", value),
        }
    }
}

pub trait BackupMemoryInterface: Sized + fmt::Debug {
    fn write(&mut self, offset: usize, value: u8);
    fn read(&self, offset: usize) -> u8;
    fn resize(&mut self, new_size: usize);
}
