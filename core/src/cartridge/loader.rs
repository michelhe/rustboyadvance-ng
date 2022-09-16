use super::super::{GBAError, GBAResult};

#[cfg(feature = "elf_support")]
use std::collections::HashMap;
use std::io::prelude::*;
use std::io::Cursor;
use std::path::Path;

#[cfg(feature = "elf_support")]
use rustboyadvance_utils::elf::{load_elf, GoblinError};
use rustboyadvance_utils::read_bin_file;
use zip::ZipArchive;

#[cfg(feature = "elf_support")]
use crate::sysbus::consts::CART_BASE;

pub enum LoadRom {
    #[cfg(feature = "elf_support")]
    Elf {
        data: Vec<u8>,
        symbols: HashMap<String, u32>,
    },
    Raw(Vec<u8>),
}
type LoadRomResult = GBAResult<LoadRom>;

#[cfg(feature = "elf_support")]
impl From<GoblinError> for GBAError {
    fn from(err: GoblinError) -> GBAError {
        GBAError::CartridgeLoadError(format!("elf parsing error: {}", err))
    }
}

#[cfg(feature = "elf_support")]
pub(super) fn try_load_elf(elf_bytes: &[u8]) -> LoadRomResult {
    let elf = load_elf(elf_bytes, CART_BASE as usize)?;
    Ok(LoadRom::Elf {
        data: elf.data,
        symbols: elf.symbols,
    })
}

fn try_load_zip(data: &[u8]) -> LoadRomResult {
    let reader = Cursor::new(data);
    let mut archive = ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name().ends_with(".gba") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(LoadRom::Raw(buf));
        }
    }
    Err(GBAError::CartridgeLoadError(
        "no .gba files found within the zip archive".to_owned(),
    ))
}

pub(super) fn load_from_file(path: &Path) -> LoadRomResult {
    let bytes = read_bin_file(path)?;

    match path.extension() {
        Some(extension) => match extension.to_str() {
            Some("zip") => try_load_zip(&bytes),
            #[cfg(feature = "elf_support")]
            Some("elf") => try_load_elf(&bytes),
            Some("gba") => Ok(LoadRom::Raw(bytes)),
            _ => {
                warn!("unknown file extension, loading as raw binary file");
                Ok(LoadRom::Raw(bytes))
            }
        },
        None => Ok(LoadRom::Raw(bytes)),
    }
}

pub(super) fn load_from_bytes(bytes: Vec<u8>) -> LoadRomResult {
    // first try as zip
    if let Ok(result) = try_load_zip(&bytes) {
        return Ok(result);
    }

    // else, try as elf
    #[cfg(feature = "elf_support")]
    {
        if let Ok(result) = try_load_elf(&bytes) {
            return Ok(result);
        }
    }

    // if everything else failed, load the rom as raw binary
    Ok(LoadRom::Raw(bytes))
}
