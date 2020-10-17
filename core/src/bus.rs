pub type Addr = u32;

pub trait Bus {
    fn read_32(&mut self, addr: Addr) -> u32 {
        self.read_16(addr) as u32 | (self.read_16(addr + 2) as u32) << 16
    }

    fn read_16(&mut self, addr: Addr) -> u16 {
        self.default_read_16(addr)
    }

    #[inline(always)]
    fn default_read_16(&mut self, addr: Addr) -> u16 {
        self.read_8(addr) as u16 | (self.read_8(addr + 1) as u16) << 8
    }

    fn read_8(&mut self, addr: Addr) -> u8;

    fn write_32(&mut self, addr: Addr, value: u32) {
        self.write_16(addr, (value & 0xffff) as u16);
        self.write_16(addr + 2, (value >> 16) as u16);
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        self.default_write_16(addr, value)
    }

    #[inline(always)]
    fn default_write_16(&mut self, addr: Addr, value: u16) {
        self.write_8(addr, (value & 0xff) as u8);
        self.write_8(addr + 1, ((value >> 8) & 0xff) as u8);
    }

    fn write_8(&mut self, addr: Addr, value: u8);

    fn get_bytes(&mut self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.read_8(b));
        }
        bytes
    }
}

/// Helper trait for reading memory as if we were an all-powerfull debugger
pub trait DebugRead: Bus {
    fn debug_read_32(&mut self, addr: Addr) -> u32 {
        self.debug_read_16(addr) as u32 | (self.debug_read_16(addr + 2) as u32) << 16
    }

    fn debug_read_16(&mut self, addr: Addr) -> u16 {
        self.debug_read_8(addr) as u16 | (self.debug_read_8(addr + 1) as u16) << 8
    }

    fn debug_read_8(&mut self, addr: Addr) -> u8;

    fn debug_get_bytes(&mut self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.debug_read_8(b));
        }
        bytes
    }
}

/// The caller is assumed to handle out of bound accesses,
/// For performance reasons, this impl trusts that 'addr' is within the array range.
impl Bus for Box<[u8]> {
    #[inline]
    fn read_8(&mut self, addr: Addr) -> u8 {
        unsafe { *self.get_unchecked(addr as usize) }
    }

    #[inline]
    fn write_8(&mut self, addr: Addr, value: u8) {
        unsafe {
            *self.get_unchecked_mut(addr as usize) = value;
        }
    }
}

impl DebugRead for Box<[u8]> {
    #[inline]
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        self[addr as usize]
    }
}
