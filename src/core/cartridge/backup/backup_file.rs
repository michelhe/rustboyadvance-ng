use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::io::SeekFrom;
use std::path::PathBuf;

use serde::de::{self, Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeStruct, Serializer};

use super::BackupMemoryInterface;
use crate::util::write_bin_file;

#[derive(Debug)]
pub struct BackupFile {
    size: usize,
    path: Option<PathBuf>,
    file: Option<File>,
    buffer: Vec<u8>,
}

impl Clone for BackupFile {
    fn clone(&self) -> Self {
        BackupFile::new(self.size, self.path.clone())
    }
}

impl Serialize for BackupFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("BackupFile", 2)?;
        state.serialize_field("size", &self.size)?;
        state.serialize_field("path", &self.path)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for BackupFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BackupFileVisitor;

        impl<'de> Visitor<'de> for BackupFileVisitor {
            type Value = BackupFile;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct BackupFile")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<BackupFile, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let size = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let path: Option<PathBuf> = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(BackupFile::new(size, path))
            }
        }

        const FIELDS: &'static [&'static str] = &["size", "path"];
        deserializer.deserialize_struct("BackupFile", FIELDS, BackupFileVisitor)
    }
}

impl BackupFile {
    pub fn new(size: usize, path: Option<PathBuf>) -> BackupFile {
        // TODO handle errors without unwrap
        let mut file: Option<File> = None;
        let buffer = if let Some(path) = &path {
            if !path.is_file() {
                write_bin_file(&path, &vec![0xff; size]).unwrap();
            }

            let mut _file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();

            let mut buffer = Vec::new();
            _file.read_to_end(&mut buffer).unwrap();
            buffer.resize(size, 0xff);

            file = Some(_file);

            buffer
        } else {
            vec![0xff; size]
        };

        BackupFile {
            size,
            path,
            file: file,
            buffer: buffer,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buffer
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    pub fn flush(&mut self) {
        if let Some(file) = &mut self.file {
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(&self.buffer).unwrap();
        }
    }
}

impl BackupMemoryInterface for BackupFile {
    fn write(&mut self, offset: usize, value: u8) {
        self.buffer[offset] = value;
        if let Some(file) = &mut self.file {
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            file.write_all(&[value]).unwrap();
        }
    }

    fn read(&self, offset: usize) -> u8 {
        self.buffer[offset]
    }

    fn resize(&mut self, new_size: usize) {
        self.size = new_size;
        self.buffer.resize(new_size, 0xff);
        self.flush();
    }
}
