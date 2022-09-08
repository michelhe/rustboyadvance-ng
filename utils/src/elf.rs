use goblin::elf;
use log::{info, warn};
use std::collections::HashMap;

pub use goblin::error::Error as GoblinError;

pub type SymbolMap = HashMap<String, u32>;

pub struct LoadedElf {
    pub data: Vec<u8>,
    pub entry: u64,
    pub symbols: SymbolMap,
}

fn read_symbols_from_elf(elf: &elf::Elf) -> SymbolMap {
    let mut symbols = SymbolMap::new();
    let strtab = &elf.strtab;
    for sym in elf.syms.iter() {
        if let Some(Ok(name)) = strtab.get(sym.st_name) {
            // TODO do I also want to save the symbol size ?
            symbols.insert(name.to_owned(), sym.st_value as u32);
        } else {
            warn!("failed to parse symbol name sym {:?}", sym);
        }
    }
    symbols
}

pub fn read_symbols(elf_bytes: &[u8]) -> goblin::error::Result<SymbolMap> {
    let elf = elf::Elf::parse(elf_bytes)?;
    Ok(read_symbols_from_elf(&elf))
}

pub fn load_elf(elf_bytes: &[u8], base: usize) -> goblin::error::Result<LoadedElf> {
    let elf = elf::Elf::parse(elf_bytes)?;

    let mut rom = vec![0; 0x200_0000];
    for phdr in &elf.program_headers {
        if phdr.p_type == elf::program_header::PT_LOAD {
            let file_range = phdr.file_range();
            let phys_range =
                (phdr.p_paddr as usize)..(phdr.p_paddr as usize + phdr.p_memsz as usize);
            let phys_range_adjusted = (phdr.p_paddr as usize - base)
                ..(phdr.p_paddr as usize + phdr.p_memsz as usize - base);

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

    Ok(LoadedElf {
        data: rom,
        entry: elf.entry,
        symbols: read_symbols_from_elf(&elf),
    })
}
