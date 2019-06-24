use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

const VIDEO_RAM_SIZE: usize = 128 * 1024;
const WORK_RAM_SIZE: usize = 256 * 1024;
const INTERNAL_RAM: usize = 32 * 1024;
const PALETTE_AM_SIZE: usize = 1 * 1024;
const OAM_SIZE: usize = 1 * 1024;
const BIOS_SIZE: usize = 16 * 1024;
const GAMEPAK_ROM_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub struct SysBus {
    bios_rom: Vec<u8>,
}

impl SysBus {
    pub fn new(bios_rom: Vec<u8>) -> SysBus {
        SysBus { bios_rom: bios_rom }
    }

    pub fn read_32(&self, addr: u32) -> u32 {
        let addr = addr as usize;
        (&self.bios_rom[addr..addr + 4])
            .read_u32::<LittleEndian>()
            .unwrap()
    }

    pub fn read_16(&self, addr: u32) -> u16 {
        let addr = addr as usize;
        (&self.bios_rom[addr..addr + 4])
            .read_u16::<LittleEndian>()
            .unwrap()
    }
}
