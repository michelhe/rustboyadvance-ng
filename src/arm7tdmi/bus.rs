use std::io;
use std::ops::Add;
use super::Addr;

pub enum MemoryAccessType {
    NonSeq,
    Seq
}

pub enum MemoryAccessWidth {
    MemoryAccess8,
    MemoryAccess16,
    MemoryAccess32
}


impl Add<MemoryAccessWidth> for MemoryAccessType {
    type Output = MemoryAccess;

    fn add(self, other: MemoryAccessWidth) -> Self::Output {
        MemoryAccess(self, other)
    }
}

pub struct MemoryAccess(MemoryAccessType, MemoryAccessWidth);

pub trait Bus {
    fn read_32(&self, addr: Addr) -> u32;
    fn read_16(&self, addr: Addr) -> u16;
    fn read_8(&self, addr: Addr) -> u8;
    fn write_32(&mut self, addr: Addr, value: u32) -> Result<(), io::Error>;
    fn write_16(&mut self, addr: Addr, value: u16) -> Result<(), io::Error>;
    fn write_8(&mut self, addr: Addr, value: u8) -> Result<(), io::Error>;

    fn get_bytes(&self, addr: Addr, size: usize) -> Option<&[u8]>;
    /// returns the number of cycles needed for this memory access
    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize;
}