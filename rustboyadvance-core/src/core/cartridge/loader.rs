use super::super::{GBAError, GBAResult};

#[cfg(feature = "elf_support")]
use std::collections::HashMap;
use std::io::prelude::*;
use std::io::Cursor;
use std::path::Path;

use crate::util::read_bin_file;
use zip::ZipArchive;

#[cfg(feature = "elf_support")]
use goblin;

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
impl From<goblin::error::Error> for GBAError {
    fn from(err: goblin::error::Error) -> GBAError {
        GBAError::CartridgeLoadError(format!("elf parsing error: {}", err))
    }
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

#[cfg(feature = "elf_support")]
fn try_load_elf(elf_bytes: &[u8]) -> LoadRomResult {
    const CART_BASE: usize = 0x0800_0000;

    let elf = goblin::elf::Elf::parse(&elf_bytes)?;

    let entry = elf.entry;
    if entry != (CART_BASE as u64) {
        return Err(GBAError::CartridgeLoadError(
            "bad elf entry point, maybe multiboot rom ?".to_owned(),
        ));
    }

    let mut rom = vec![0; 0x200_0000];
    for phdr in &elf.program_headers {
        if phdr.p_type == goblin::elf::program_header::PT_LOAD {
            let file_range = phdr.file_range();
            let phys_range =
                (phdr.p_paddr as usize)..(phdr.p_paddr as usize + phdr.p_memsz as usize);
            let phys_range_adjusted = (phdr.p_paddr as usize - CART_BASE)
                ..(phdr.p_paddr as usize + phdr.p_memsz as usize - CART_BASE);

            if phys_range_adjusted.start + (phdr.p_filesz as usize) >= rom.len() {
                warn!("ELF: skipping program header {:?}", phdr);
                continue;
            }

            info!(
                "ELF: loading segment phdr: {:?} range {:#x?} vec range {:#x?}",
                phdr, file_range, phys_range,
            );

            let src = &elf_bytes[file_range];
            let dst = &mut rom[phys_range_adjusted];
            dst.copy_from_slice(src);
        }
    }

    let mut symbols = HashMap::new();

    let strtab = elf.strtab;
    for sym in elf.syms.iter() {
        if let Some(Ok(name)) = strtab.get(sym.st_name) {
            // TODO do I also want to save the symbol size ?
            symbols.insert(name.to_owned(), sym.st_value as u32);
        } else {
            warn!("failed to parse symbol name sym {:?}", sym);
        }
    }

    Ok(LoadRom::Elf {
        data: rom,
        symbols: symbols,
    })
}

pub(super) fn load_from_file(path: &Path) -> LoadRomResult {
    let bytes = read_bin_file(path)?;

    match path.extension() {
        Some(extension) => match extension.to_str() {
            Some("zip") => try_load_zip(&bytes),
            #[cfg(feature = "elf_support")]
            Some("elf") => try_load_elf(&bytes),
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
