use std::io;

#[macro_use]
extern crate clap;

use clap::{App, ArgMatches};

extern crate rustboyadvance_ng;

use rustboyadvance_ng::arm7tdmi;
use rustboyadvance_ng::debugger::{Debugger, DebuggerError};
use rustboyadvance_ng::sysbus::SysBus;
use rustboyadvance_ng::util::read_bin_file;

#[derive(Debug)]
pub enum GBAError {
    IO(io::Error),
    ArmDecodeError(arm7tdmi::arm::ArmDecodeError),
    CpuError(arm7tdmi::CpuError),
    DebuggerError(DebuggerError),
}

pub type GBAResult<T> = Result<T, GBAError>;

impl From<io::Error> for GBAError {
    fn from(err: io::Error) -> GBAError {
        GBAError::IO(err)
    }
}

impl From<arm7tdmi::arm::ArmDecodeError> for GBAError {
    fn from(err: arm7tdmi::arm::ArmDecodeError) -> GBAError {
        GBAError::ArmDecodeError(err)
    }
}

impl From<arm7tdmi::CpuError> for GBAError {
    fn from(err: arm7tdmi::CpuError) -> GBAError {
        GBAError::CpuError(err)
    }
}

impl From<DebuggerError> for GBAError {
    fn from(err: DebuggerError) -> GBAError {
        GBAError::DebuggerError(err)
    }
}

fn run_debug(matches: &ArgMatches) -> GBAResult<()> {
    let bios_bin = read_bin_file(matches.value_of("bios").unwrap_or_default())?;
    let rom_bin = read_bin_file(matches.value_of("game_rom").unwrap())?;

    let sysbus = SysBus::new(bios_bin, rom_bin);
    let mut core = arm7tdmi::cpu::Core::new();
    core.reset();
    core.set_verbose(true);
    let mut debugger = Debugger::new(core, sysbus);

    println!("starting debugger...");
    debugger.repl()?;
    println!("ending debugger...");

    Ok(())
}

fn main() {
    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    let result = match matches.subcommand() {
        ("debug", Some(m)) => run_debug(m),
        _ => Ok(()),
    };

    if let Err(err) = result {
        println!("Got an error: {:?}", err);
    }
}
