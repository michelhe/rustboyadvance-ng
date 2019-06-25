use std::convert::TryFrom;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind;

#[macro_use]
extern crate enum_primitive_derive;
extern crate num_traits;

extern crate bit;

extern crate byteorder;
use byteorder::{LittleEndian, ReadBytesExt};

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

    let mut rdr = io::Cursor::new(bin);
    loop {
        let value: u32 = match rdr.read_u32::<LittleEndian>() {
            Ok(value) => Ok(value),
            Err(e) => match e.kind() {
                ErrorKind::UnexpectedEof => {
                    break;
                }
                _ => Err(e),
            },
        }?;
        let addr = (rdr.position() - 4) as u32;
        print!("{:8x}:\t{:08x} \t", addr, value);
        match arm::ArmInstruction::try_from((value, addr)) {
            Ok(insn) => println!("{}", insn),
            Err(_) => println!("<UNDEFINED>"),
        };
    }

    Ok(())
}

fn run_debug(matches: &ArgMatches) -> GBAResult<()> {
    let gba_bios_path = matches.value_of("BIOS").unwrap_or_default();
    println!("Loading BIOS: {}", gba_bios_path);
    let bios_bin = read_bin_file(gba_bios_path)?;

    let mut sysbus = SysBus::new(bios_bin);
    let mut core = cpu::Core::new();
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
