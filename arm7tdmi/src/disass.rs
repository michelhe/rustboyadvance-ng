use std::env;
use std::io;
use std::io::Cursor;
use std::io::prelude::*;
use std::fs::File;
use std::convert::TryFrom;

extern crate byteorder;
use byteorder::{LittleEndian, ReadBytesExt};

extern crate arm7tdmi;

use arm7tdmi::arm::arm_isa::ArmInstruction;

#[derive(Debug)]
pub enum DisassemblerError {
    IO(io::Error),
}

impl From<io::Error> for DisassemblerError {
    fn from(err: io::Error) -> DisassemblerError {
        DisassemblerError::IO(err)
    }
}

fn read_file(filename: &str) -> Result<Vec<u8>, DisassemblerError> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

fn main() {
    let filename = match env::args().nth(1) {
        Some(filename) => filename,
        None => panic!("usage: {} <file> <n>", env::args().nth(0).unwrap())
    };

    // let num_instructions = match env::args().nth(2) {
    //     Some(n) => n,
    //     None => panic!("usage: {} <file> <n>", env::args().nth(0).unwrap())
    // }.parse::<usize>();

    let buf = match read_file(&filename) {
        Ok(buf) => buf,
        Err(e) => panic!(e)
    };

    let mut rdr = Cursor::new(buf);
    loop {
        let value: u32 = match rdr.read_u32::<LittleEndian>() {
            Ok(v) => v,
            Err(err) => {
                panic!("got an error {:?}", err);
            }
        };
        let addr = (rdr.position() - 4) as u32;
        print!("{:8x}:\t{:08x} \t", addr, value);
        match ArmInstruction::try_from((value, addr)) {
            Ok(insn) => println!("{}", insn),
            Err(_) => println!("<UNDEFINED>")
        }
    }
}
