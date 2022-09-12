use std::fmt;
use std::str::FromStr;

mod backup_file;
pub use backup_file::BackupFile;
pub mod eeprom;
pub mod flash;

#[derive(Debug, Primitive, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
pub enum BackupType {
    Eeprom = 0,
    Sram = 1,
    Flash = 2,
    Flash512 = 3,
    Flash1M = 4,
    AutoDetect = 5,
}

impl FromStr for BackupType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use BackupType::*;
        match s {
            "autodetect" => Ok(AutoDetect),
            "sram" => Ok(Sram),
            "flash128k" => Ok(Flash1M),
            "flash64k" => Ok(Flash512),
            "eeprom" => Ok(Eeprom),
            _ => Err(format!("{} is not a valid save type", s)),
        }
    }
}

pub trait BackupMemoryInterface: Sized + fmt::Debug {
    fn write(&mut self, offset: usize, value: u8);
    fn read(&self, offset: usize) -> u8;
    fn resize(&mut self, new_size: usize);
}
