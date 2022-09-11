use crate::gdb::{copy_range_to_buf, target::MemoryGdbInterface};
use crate::memory::{Addr, BusIO, DebugRead, MemoryAccess, MemoryInterface};

/// Simple wrapper around a bytearray for memory access
/// For use by tests and examples of this crate.
pub struct SimpleMemory {
    data: Box<[u8]>,
}

impl SimpleMemory {
    pub fn new(capacity: usize) -> SimpleMemory {
        SimpleMemory {
            data: vec![0; capacity].into_boxed_slice(),
        }
    }
    pub fn load_program(&mut self, program: &[u8]) {
        self.data[..program.len()].copy_from_slice(program);
    }
}

impl MemoryInterface for SimpleMemory {
    #[inline]
    fn load_8(&mut self, addr: u32, _access: MemoryAccess) -> u8 {
        self.read_8(addr)
    }

    #[inline]
    fn load_16(&mut self, addr: u32, _access: MemoryAccess) -> u16 {
        self.read_16(addr & !1)
    }

    #[inline]
    fn load_32(&mut self, addr: u32, _access: MemoryAccess) -> u32 {
        self.read_32(addr & !3)
    }

    fn store_8(&mut self, addr: u32, value: u8, _access: MemoryAccess) {
        self.write_8(addr, value);
    }

    fn store_16(&mut self, addr: u32, value: u16, _access: MemoryAccess) {
        self.write_16(addr & !1, value);
    }

    fn store_32(&mut self, addr: u32, value: u32, _access: MemoryAccess) {
        self.write_32(addr & !3, value);
    }

    fn idle_cycle(&mut self) {}
}

impl BusIO for SimpleMemory {
    fn read_8(&mut self, addr: Addr) -> u8 {
        *self.data.get(addr as usize).unwrap_or(&0)
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        if let 0..=0x3FFF = addr {
            self.data[addr as usize] = value;
        }
    }
}

impl DebugRead for SimpleMemory {
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        *self.data.get(addr as usize).unwrap_or(&0)
    }
}

impl MemoryGdbInterface for SimpleMemory {
    fn memory_map_xml(&self, offset: u64, length: usize, buf: &mut [u8]) -> usize {
        let memory_map = format!(
            r#"<?xml version="1.0"?>
    <!DOCTYPE memory-map
        PUBLIC "+//IDN gnu.org//DTD GDB Memory Map V1.0//EN"
                "http://sourceware.org/gdb/gdb-memory-map.dtd">
    <memory-map>
        <memory type="ram" start="0x0" length="{}"/>
    </memory-map>"#,
            self.data.len()
        );
        copy_range_to_buf(memory_map.trim().as_bytes(), offset, length, buf)
    }
}
