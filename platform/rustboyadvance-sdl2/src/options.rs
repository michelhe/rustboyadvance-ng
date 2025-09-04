use std::path::PathBuf;

use clap::Parser;
use rustboyadvance_core::{
    cartridge::{BackupType, GamepakBuilder},
    prelude::Cartridge,
};

#[derive(Parser, Debug)]
#[command(name = "rustboyadvance-sdl2")]
pub struct Options {
    /// Rom file to emulate, may be a raw dump from a cartridge or a compiled ELF file
    #[arg(name = "ROM")]
    pub rom: PathBuf,

    /// Bios file to use
    #[arg(long, default_value = "gba_bios.bin")]
    pub bios: PathBuf,

    /// Skip running the bios boot animation and jump straight to the ROM
    #[arg(long)]
    pub skip_bios: bool,

    /// Do not output sound
    #[arg(long)]
    pub _silent: bool,

    /// Initalize gdbserver and wait for a connection from gdb
    #[arg(short = 'd', long)]
    pub gdbserver: bool,

    #[arg(long = "port", default_value = "1337")]
    pub gdbserver_port: u16,

    /// Force emulation of RTC, use for games that have RTC but the emulator fails to detect
    #[arg(long)]
    pub rtc: bool,

    /// Override save type, useful for troublemaking games that fool the auto detection
    #[arg(long, default_value = "autodetect", value_enum)]
    pub save_type: Option<BackupType>,

    #[cfg(feature = "debugger")]
    #[arg(long, default_value = None)]
    pub script_file: Option<String>,
}

type DynError = Box<dyn std::error::Error>;

impl Options {
    pub fn cartridge_from_opts(&self) -> Result<Cartridge, DynError> {
        let mut builder = GamepakBuilder::new()
            .save_type(self.save_type.unwrap_or(BackupType::AutoDetect))
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
}
