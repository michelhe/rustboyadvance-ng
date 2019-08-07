use super::Addr;

pub trait Bus {
    fn read_32(&self, addr: Addr) -> u32;
    fn read_16(&self, addr: Addr) -> u16;
    fn read_8(&self, addr: Addr) -> u8;
    fn write_32(&mut self, addr: Addr, value: u32);
    fn write_16(&mut self, addr: Addr, value: u16);
    fn write_8(&mut self, addr: Addr, value: u8);

    fn get_bytes(&self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.read_8(b));
        }
        bytes
    }
}
