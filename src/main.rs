use std::fs::File;
use std::io;
use std::io::prelude::*;

#[macro_use]
extern crate enum_primitive_derive;
extern crate num_traits;

extern crate bit;

extern crate byteorder;

#[macro_use]
extern crate clap;
use clap::{App, ArgMatches};

extern crate rustyline;

extern crate nom;

extern crate colored; // not needed in Rust 2018

pub mod sysbus;
use sysbus::SysBus;

mod arm7tdmi;
use arm7tdmi::arm;
use arm7tdmi::cpu;

mod debugger;
use debugger::{Debugger, DebuggerError};

mod disass;
use disass::Disassembler;

#[derive(Debug)]
pub enum GBAError {
    IO(io::Error),
    ArmDecodeError(arm::ArmDecodeError),
    CpuError(cpu::CpuError),
    DebuggerError(DebuggerError)
}

pub type GBAResult<T> = Result<T, GBAError>;

impl From<io::Error> for GBAError {
    fn from(err: io::Error) -> GBAError {
        GBAError::IO(err)
    }
}

impl From<arm::ArmDecodeError> for GBAError {
    fn from(err: arm::ArmDecodeError) -> GBAError {
        GBAError::ArmDecodeError(err)
    }
}

impl From<cpu::CpuError> for GBAError {
    fn from(err: cpu::CpuError) -> GBAError {
        GBAError::CpuError(err)
    }
}

impl From<DebuggerError> for GBAError {
    fn from(err: DebuggerError) -> GBAError {
        GBAError::DebuggerError(err)
    }
}


fn read_bin_file(filename: &str) -> GBAResult<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

fn run_disass(matches: &ArgMatches) -> GBAResult<()> {
    let input = matches.value_of("INPUT").unwrap();
    let bin = read_bin_file(&input)?;

    let disassembler = Disassembler::new(0, &bin);
    for line in disassembler {
        println!("{}", line)
    }
    Ok(())
}

fn run_debug(matches: &ArgMatches) -> GBAResult<()> {
    let gba_bios_path = matches.value_of("BIOS").unwrap_or_default();
    println!("Loading BIOS: {}", gba_bios_path);
    let bios_bin = read_bin_file(gba_bios_path)?;

    let sysbus = SysBus::new(bios_bin);
    let mut core = cpu::Core::new();
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
        ("disass", Some(m)) => run_disass(m),
        _ => Ok(()),
    };

    if let Err(err) = result {
        println!("Got an error: {:?}", err);
    }
}
