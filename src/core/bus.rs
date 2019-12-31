pub type Addr = u32;

pub trait Bus {
    fn read_32(&self, addr: Addr) -> u32 {
        self.read_16(addr) as u32 | (self.read_16(addr + 2) as u32) << 16
    }

    fn read_16(&self, addr: Addr) -> u16 {
        self.read_8(addr) as u16 | (self.read_8(addr + 1) as u16) << 8
    }

    fn read_8(&self, addr: Addr) -> u8;

    fn write_32(&mut self, addr: Addr, value: u32) {
        self.write_16(addr, (value & 0xffff) as u16);
        self.write_16(addr + 2, (value >> 16) as u16);
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        self.write_8(addr, (value & 0xff) as u8);
        self.write_8(addr + 1, ((value >> 8) & 0xff) as u8);
    }

    fn write_8(&mut self, addr: Addr, value: u8);

    fn get_bytes(&self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.read_8(b));
        }
        bytes
    }
}
