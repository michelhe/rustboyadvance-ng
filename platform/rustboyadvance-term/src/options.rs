use std::{io, path::PathBuf};

use rustboyadvance_core::{
    cartridge::{BackupType, GamepakBuilder},
    prelude::Cartridge,
};
use rustboyadvance_utils::read_bin_file;
use structopt::StructOpt;

const SAVE_TYPE_POSSIBLE_VALUES: &[&str] =
    &["sram", "flash128k", "flash64k", "eeprom", "autodetect"];

#[derive(StructOpt, Debug)]
#[structopt(name = "rustboyadvance-sdl2")]
pub struct Options {
    /// Rom file to emulate, may be a raw dump from a cartridge or a compiled ELF file
    #[structopt(name = "ROM", parse(from_os_str))]
    pub rom: PathBuf,

    /// Bios file to use
    #[structopt(long, parse(from_os_str), default_value = "gba_bios.bin")]
    pub bios: PathBuf,

    /// Skip running the bios boot animation and jump straight to the ROM
    #[structopt(long)]
    pub skip_bios: bool,

    /// Do not output sound
    #[structopt(long)]
    pub silent: bool,

    /// Initalize gdbserver and wait for a connection from gdb
    #[structopt(short = "d", long)]
    pub gdbserver: bool,

    #[structopt(long = "port", default_value = "1337")]
    pub gdbserver_port: u16,

    /// Force emulation of RTC, use for games that have RTC but the emulator fails to detect
    #[structopt(long)]
    pub rtc: bool,

    /// Override save type, useful for troublemaking games that fool the auto detection
    #[structopt(long, default_value = "autodetect", possible_values = SAVE_TYPE_POSSIBLE_VALUES)]
    pub save_type: BackupType,
}

type DynError = Box<dyn std::error::Error>;

impl Options {
    pub fn cartridge_from_opts(&self) -> Result<Cartridge, DynError> {
        let mut builder = GamepakBuilder::new()
            .save_type(self.save_type)
            .file(&self.rom);
        if self.rtc {
            builder = builder.with_rtc();
        }
        Ok(builder.build()?)
    }

    pub fn savestate_path(&self) -> PathBuf {
        self.rom.with_extension("savestate")
    }

    pub fn rom_name(&self) -> &str {
        self.rom.file_name().unwrap().to_str().unwrap()
    }

    pub fn read_rom(&self) -> Result<Vec<u8>, std::io::Error> {
        read_bin_file(&self.rom)
    }
}
