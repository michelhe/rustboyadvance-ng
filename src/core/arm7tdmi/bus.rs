use std::fmt;
use std::ops::Add;

use super::Addr;

#[derive(Debug)]
pub enum MemoryAccessType {
    NonSeq,
    Seq,
}

impl fmt::Display for MemoryAccessType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MemoryAccessType::NonSeq => "N",
                MemoryAccessType::Seq => "S",
            }
        )
    }
}

#[derive(Debug)]
pub enum MemoryAccessWidth {
    MemoryAccess8,
    MemoryAccess16,
    MemoryAccess32,
}

impl Add<MemoryAccessWidth> for MemoryAccessType {
    type Output = MemoryAccess;

    fn add(self, other: MemoryAccessWidth) -> Self::Output {
        MemoryAccess(self, other)
    }
}

#[derive(Debug)]
pub struct MemoryAccess(pub MemoryAccessType, pub MemoryAccessWidth);

impl fmt::Display for MemoryAccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-Cycle ({:?})", self.0, self.1)
    }
}

pub trait Bus {
    fn read_32(&self, addr: Addr) -> u32;
    fn read_16(&self, addr: Addr) -> u16;
    fn read_8(&self, addr: Addr) -> u8;
    fn write_32(&mut self, addr: Addr, value: u32);
    fn write_16(&mut self, addr: Addr, value: u16);
    fn write_8(&mut self, addr: Addr, value: u8);

    /// returns the number of cycles needed for this memory access
    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize;

    fn get_bytes(&self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.read_8(b));
        }
        bytes
    }
}
