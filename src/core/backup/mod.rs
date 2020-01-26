use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::io::SeekFrom;
use std::path::PathBuf;

use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::util::write_bin_file;

pub mod flash;

pub const BACKUP_FILE_EXT: &'static str = "sav";

#[derive(Debug, Primitive, Serialize, Deserialize, Clone)]
pub enum BackupType {
    Eeprom = 0,
    Sram = 1,
    Flash = 2,
    Flash512 = 3,
    Flash1M = 4,
}

#[derive(Debug)]
pub struct BackupMemory {
    size: usize,
    path: PathBuf,
    file: File,
    buffer: Vec<u8>,
}

impl Clone for BackupMemory {
    fn clone(&self) -> Self {
        BackupMemory::new(self.size, self.path.clone())
    }
}

impl Serialize for BackupMemory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("BackupMemory", 2)?;
        state.serialize_field("size", &self.size)?;
        state.serialize_field("path", &self.path)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for BackupMemory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BackupMemoryVisitor;

        impl<'de> Visitor<'de> for BackupMemoryVisitor {
            type Value = BackupMemory;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct BackupMemory")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<BackupMemory, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let size = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let path: String = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(BackupMemory::new(size, PathBuf::from(path)))
            }
        }

        const FIELDS: &'static [&'static str] = &["size", "path"];
        deserializer.deserialize_struct("BackupMemory", FIELDS, BackupMemoryVisitor)
    }
}

impl BackupMemory {
    pub fn new(size: usize, path: PathBuf) -> BackupMemory {
        // TODO handle errors without unwrap
        if !path.is_file() {
            write_bin_file(&path, &vec![0xff; size]).unwrap();
        };

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).unwrap();
        buffer.resize(size, 0xff);

        BackupMemory {
            size,
            path,
            file: file,
            buffer: buffer,
        }
    }

    pub fn write(&mut self, offset: usize, value: u8) {
        self.buffer[offset] = value;
        self.file.seek(SeekFrom::Start(offset as u64)).unwrap();
        self.file.write_all(&[value]).unwrap();
    }

    pub fn read(&self, offset: usize) -> u8 {
        self.buffer[offset]
    }
}
