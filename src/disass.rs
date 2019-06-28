use std::convert::TryFrom;

use std::io::ErrorKind;
use std::io::{Cursor, Seek, SeekFrom};

use byteorder::{LittleEndian, ReadBytesExt};

use super::arm7tdmi::arm;

pub struct Disassembler<'a> {
    base: u32,
    rdr: Cursor<&'a [u8]>,
}

impl<'a> Disassembler<'a> {
    pub fn new(base: u32, bin: &'a [u8]) -> Disassembler {
        Disassembler {
            base: base,
            rdr: Cursor::new(bin),
        }
    }
}

impl<'a> Seek for Disassembler<'a> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.rdr.seek(pos)
    }
}

impl<'a> Iterator for Disassembler<'a> {
    type Item = (u32, String);

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();
        let value: u32 = match self.rdr.read_u32::<LittleEndian>() {
            Ok(value) => value,
            Err(e) => match e.kind() {
                ErrorKind::UnexpectedEof => {
                    return None;
                }
                _ => panic!("unexpected error"),
            },
        };

        let addr = self.base + (self.rdr.position() - 4) as u32;
        line.push_str(&format!("{:8x}:\t{:08x} \t", addr, value));

        match arm::ArmInstruction::try_from((value, addr)) {
            Ok(insn) => line.push_str(&format!("{}", insn)),
            Err(_) => line.push_str("<UNDEFINED>"),
        };

        Some((addr, line))
    }
}
