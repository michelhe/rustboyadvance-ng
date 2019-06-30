use std::io;

use super::arm7tdmi::Addr;
use super::arm7tdmi::bus::{Bus, MemoryAccess, MemoryAccessType, MemoryAccessWidth};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

const VIDEO_RAM_SIZE: usize = 128 * 1024;
const WORK_RAM_SIZE: usize = 256 * 1024;
const INTERNAL_RAM: usize = 32 * 1024;
const PALETTE_AM_SIZE: usize = 1 * 1024;
const OAM_SIZE: usize = 1 * 1024;
const BIOS_SIZE: usize = 16 * 1024;
const GAMEPAK_ROM_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug)]
struct BiosROM(Vec<u8>);

impl Bus for BiosROM {
    fn read_32(&self, addr: Addr) -> u32 {
        let addr = addr as usize;
        (&self.0[addr..addr + 4])
            .read_u32::<LittleEndian>()
            .unwrap()
    }

    fn read_16(&self, addr: Addr) -> u16 {
        let addr = addr as usize;
        (&self.0[addr..addr + 4])
            .read_u16::<LittleEndian>()
            .unwrap()
    }

    fn read_8(&self, addr: Addr) -> u8 {
        self.0[addr as usize]
    }

    fn write_32(&mut self, addr: Addr, value: u32) -> Result<(), io::Error> {
        let mut wrt =  io::Cursor::new(&mut self.0);
        wrt.set_position(addr as u64);
        wrt.write_u32::<LittleEndian>(value)
    }

    fn write_16(&mut self, addr: Addr, value: u16) -> Result<(), io::Error> {
        let mut wrt =  io::Cursor::new(&mut self.0);
        wrt.set_position(addr as u64);
        wrt.write_u16::<LittleEndian>(value)
    }

    fn write_8(&mut self, addr: Addr, value: u8) -> Result<(), io::Error> {
        let mut wrt = io::Cursor::new(&mut self.0);
        wrt.write_u8(value)
    }

    fn get_bytes(&self, addr: Addr, size: usize) -> Option<&[u8]> {
        let addr = addr as usize;
        if addr + size > self.0.len() {
            None
        } else {
            Some(&self.0[addr..addr + size])
        }
    }
    
    fn get_cycles(&self, _addr: Addr, _access: MemoryAccess) -> usize {
        1
    }
}

#[derive(Debug)]
enum SysBusDevice {
    BiosROM(BiosROM)
}

#[derive(Debug)]
pub struct SysBus {
    bios: BiosROM
}

impl SysBus {
    pub fn new(bios_rom: Vec<u8>) -> SysBus {
        SysBus { bios: BiosROM(bios_rom) }
    }

    fn map(&self, addr: Addr) -> & impl Bus {
        match addr as usize {
            0...BIOS_SIZE => &self.bios,
            _ => panic!("unmapped address")
        }
    }

    fn map_mut(&mut self, addr: Addr) -> &mut impl Bus {
        match addr as usize {
            0...BIOS_SIZE => &mut self.bios,
            _ => panic!("unmapped address")
        }
    }
}

impl Bus for SysBus {
    fn read_32(&self, addr: Addr) -> u32 {
        self.map(addr).read_32(addr)
    }

    fn read_16(&self, addr: Addr) -> u16 {
        self.map(addr).read_16(addr)
    }

    fn read_8(&self, addr: Addr) -> u8 {
        self.map(addr).read_8(addr)
    }

    fn write_32(&mut self, addr: Addr, value: u32) -> Result<(), io::Error> {
        self.map_mut(addr).write_32(addr, value)
    }

    fn write_16(&mut self, addr: Addr, value: u16) -> Result<(), io::Error> {
        self.map_mut(addr).write_16(addr, value)
    }

    fn write_8(&mut self, addr: Addr, value: u8) -> Result<(), io::Error> {
        self.map_mut(addr).write_8(addr, value)
    }


    fn get_bytes(&self, addr: Addr, size: usize) -> Option<&[u8]> {
        self.map(addr).get_bytes(addr, size)
    }

    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize {
        self.map(addr).get_cycles(addr, access)
    }
}
