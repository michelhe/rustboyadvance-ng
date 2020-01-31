use std::fmt;

mod backup_file;
pub use backup_file::BackupFile;
pub mod eeprom;
pub mod flash;

#[derive(Debug, Primitive, Serialize, Deserialize, Copy, Clone, PartialEq)]
pub enum BackupType {
    Eeprom = 0,
    Sram = 1,
    Flash = 2,
    Flash512 = 3,
    Flash1M = 4,
    AutoDetect = 5,
}

pub trait BackupMemoryInterface: Sized + fmt::Debug {
    fn write(&mut self, offset: usize, value: u8);
    fn read(&self, offset: usize) -> u8;
}
