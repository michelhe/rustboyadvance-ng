use std::io;

use crate::cartridge::Cartridge;

use super::arm7tdmi::bus::{Bus, MemoryAccess, MemoryAccessWidth};
use super::arm7tdmi::Addr;

const VIDEO_RAM_SIZE: usize = 128 * 1024;
const WORK_RAM_SIZE: usize = 256 * 1024;
const INTERNAL_RAM: usize = 32 * 1024;
const PALETTE_RAM_SIZE: usize = 1 * 1024;
const OAM_SIZE: usize = 1 * 1024;

#[derive(Debug)]
pub struct BoxedMemory(Box<[u8]>, WaitState);

impl BoxedMemory {
    pub fn new(boxed_slice: Box<[u8]>) -> BoxedMemory {
        BoxedMemory(boxed_slice, Default::default())
    }

    pub fn new_with_waitstate(boxed_slice: Box<[u8]>, ws: WaitState) -> BoxedMemory {
        BoxedMemory(boxed_slice, ws)
    }
}

#[derive(Debug)]
pub struct WaitState {
    pub access8: usize,
    pub access16: usize,
    pub access32: usize,
}

impl WaitState {
    pub fn new(access8: usize, access16: usize, access32: usize) -> WaitState {
        WaitState {
            access8,
            access16,
            access32,
        }
    }
}

impl Default for WaitState {
    fn default() -> WaitState {
        WaitState::new(1, 1, 1)
    }
}

impl Bus for BoxedMemory {
    fn get_bytes(&self, addr: Addr) -> &[u8] {
        &self.0[addr as usize..]
    }

    fn get_bytes_mut(&mut self, addr: Addr) -> &mut [u8] {
        &mut self.0[addr as usize..]
    }

    fn get_cycles(&self, _addr: Addr, access: MemoryAccess) -> usize {
        match access.1 {
            MemoryAccessWidth::MemoryAccess8 => self.1.access8,
            MemoryAccessWidth::MemoryAccess16 => self.1.access16,
            MemoryAccessWidth::MemoryAccess32 => self.1.access32,
        }
    }
}

#[derive(Debug)]
pub struct SysBus {
    bios: BoxedMemory,
    onboard_work_ram: BoxedMemory,
    internal_work_ram: BoxedMemory,
    /// Currently model the IOMem as regular buffer, later make it into something more sophisticated.
    ioregs: BoxedMemory,
    palette_ram: BoxedMemory,
    vram: BoxedMemory,
    oam: BoxedMemory,
    gamepak: Cartridge,
}

impl SysBus {
    pub fn new(bios_rom: Vec<u8>, gamepak: Cartridge) -> SysBus {
        SysBus {
            bios: BoxedMemory::new(bios_rom.into_boxed_slice()),
            onboard_work_ram: BoxedMemory::new_with_waitstate(
                vec![0; WORK_RAM_SIZE].into_boxed_slice(),
                WaitState::new(3, 3, 6),
            ),
            internal_work_ram: BoxedMemory::new(vec![0; INTERNAL_RAM].into_boxed_slice()),
            ioregs: BoxedMemory::new(vec![0; 1024].into_boxed_slice()),
            palette_ram: BoxedMemory::new_with_waitstate(
                vec![0; PALETTE_RAM_SIZE].into_boxed_slice(),
                WaitState::new(1, 1, 2),
            ),
            vram: BoxedMemory::new_with_waitstate(
                vec![0; VIDEO_RAM_SIZE].into_boxed_slice(),
                WaitState::new(1, 1, 2),
            ),
            oam: BoxedMemory::new(vec![0; OAM_SIZE].into_boxed_slice()),
            gamepak: gamepak,
        }
    }

    fn map(&self, addr: Addr) -> &Bus {
        match addr as usize {
            0x0000_0000...0x0000_3fff => &self.bios,
            0x0200_0000...0x0203_ffff => &self.onboard_work_ram,
            0x0300_0000...0x0300_7fff => &self.internal_work_ram,
            0x0400_0000...0x0400_03fe => &self.ioregs,
            0x0500_0000...0x0500_03ff => &self.palette_ram,
            0x0600_0000...0x0601_7fff => &self.vram,
            0x0700_0000...0x0700_03ff => &self.oam,
            0x0800_0000...0x09ff_ffff => &self.gamepak,
            _ => panic!("unmapped address @0x{:08x}", addr),
        }
    }

    /// TODO proc-macro for generating this function
    fn map_mut(&mut self, addr: Addr) -> &mut Bus {
        match addr as usize {
            0x0000_0000...0x0000_3fff => &mut self.bios,
            0x0200_0000...0x0203_ffff => &mut self.onboard_work_ram,
            0x0300_0000...0x0300_7fff => &mut self.internal_work_ram,
            0x0400_0000...0x0400_03fe => &mut self.ioregs,
            0x0500_0000...0x0500_03ff => &mut self.palette_ram,
            0x0600_0000...0x0601_7fff => &mut self.vram,
            0x0700_0000...0x0700_03ff => &mut self.oam,
            0x0800_0000...0x09ff_ffff => &mut self.gamepak,
            _ => panic!("unmapped address @0x{:08x}", addr),
        }
    }
}

impl Bus for SysBus {
    fn read_32(&self, addr: Addr) -> u32 {
        self.map(addr).read_32(addr & 0xff_ffff)
    }

    fn read_16(&self, addr: Addr) -> u16 {
        self.map(addr).read_16(addr & 0xff_ffff)
    }

    fn read_8(&self, addr: Addr) -> u8 {
        self.map(addr).read_8(addr & 0xff_ffff)
    }

    fn write_32(&mut self, addr: Addr, value: u32) -> Result<(), io::Error> {
        self.map_mut(addr).write_32(addr & 0xff_ffff, value)
    }

    fn write_16(&mut self, addr: Addr, value: u16) -> Result<(), io::Error> {
        self.map_mut(addr).write_16(addr & 0xff_ffff, value)
    }

    fn write_8(&mut self, addr: Addr, value: u8) -> Result<(), io::Error> {
        self.map_mut(addr).write_8(addr & 0xff_ffff, value)
    }

    fn get_bytes(&self, addr: Addr) -> &[u8] {
        self.map(addr).get_bytes(addr & 0xff_ffff)
    }

    fn get_bytes_mut(&mut self, addr: Addr) -> &mut [u8] {
        self.map_mut(addr).get_bytes_mut(addr & 0xff_ffff)
    }

    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize {
        self.map(addr).get_cycles(addr & 0xff_ffff, access)
    }
}
