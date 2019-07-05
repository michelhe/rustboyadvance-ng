use std::fmt;
use std::io;
use std::ops::Add;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

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
    fn read_32(&self, addr: Addr) -> u32 {
        self.get_bytes(addr).read_u32::<LittleEndian>().unwrap()
    }

    fn read_16(&self, addr: Addr) -> u16 {
        self.get_bytes(addr).read_u16::<LittleEndian>().unwrap()
    }

    fn read_8(&self, addr: Addr) -> u8 {
        self.get_bytes(addr)[0]
    }

    fn write_32(&mut self, addr: Addr, value: u32) -> Result<(), io::Error> {
        self.get_bytes_mut(addr).write_u32::<LittleEndian>(value)
    }

    fn write_16(&mut self, addr: Addr, value: u16) -> Result<(), io::Error> {
        self.get_bytes_mut(addr).write_u16::<LittleEndian>(value)
    }

    fn write_8(&mut self, addr: Addr, value: u8) -> Result<(), io::Error> {
        self.get_bytes_mut(addr).write_u8(value)
    }
    /// Return a slice of bytes
    fn get_bytes(&self, addr: Addr) -> &[u8];

    /// Return a mutable slice of bytes
    fn get_bytes_mut(&mut self, addr: Addr) -> &mut [u8];

    /// returns the number of cycles needed for this memory access
    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize;
}
