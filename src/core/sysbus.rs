use std::cell::RefCell;
use std::rc::Rc;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use super::arm7tdmi::bus::{Bus, MemoryAccess, MemoryAccessWidth};
use super::arm7tdmi::Addr;
use super::gba::IoDevices;
use super::{cartridge::Cartridge, ioregs::IoRegs};

const VIDEO_RAM_SIZE: usize = 128 * 1024;
const WORK_RAM_SIZE: usize = 256 * 1024;
const INTERNAL_RAM_SIZE: usize = 32 * 1024;
const PALETTE_RAM_SIZE: usize = 1 * 1024;
const OAM_SIZE: usize = 1 * 1024;

#[derive(Debug)]
pub struct BoxedMemory {
    pub mem: Box<[u8]>,
    ws: WaitState,
    mask: u32,
}

impl BoxedMemory {
    pub fn new(boxed_slice: Box<[u8]>, mask: u32) -> BoxedMemory {
        BoxedMemory {
            mem: boxed_slice,
            mask: mask,
            ws: WaitState::default(),
        }
    }

    pub fn new_with_waitstate(boxed_slice: Box<[u8]>, mask: u32, ws: WaitState) -> BoxedMemory {
        BoxedMemory {
            mem: boxed_slice,
            mask: mask,
            ws: ws,
        }
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
    fn read_32(&self, addr: Addr) -> u32 {
        (&self.mem[(addr & self.mask) as usize..])
            .read_u32::<LittleEndian>()
            .unwrap()
    }

    fn read_16(&self, addr: Addr) -> u16 {
        (&self.mem[(addr & self.mask) as usize..])
            .read_u16::<LittleEndian>()
            .unwrap()
    }

    fn read_8(&self, addr: Addr) -> u8 {
        (&self.mem[(addr & self.mask) as usize..])[0]
    }

    fn write_32(&mut self, addr: Addr, value: u32) {
        (&mut self.mem[(addr & self.mask) as usize..])
            .write_u32::<LittleEndian>(value)
            .unwrap()
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        (&mut self.mem[(addr & self.mask) as usize..])
            .write_u16::<LittleEndian>(value)
            .unwrap()
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        (&mut self.mem[(addr & self.mask) as usize..])
            .write_u8(value)
            .unwrap()
    }

    fn get_cycles(&self, _addr: Addr, access: MemoryAccess) -> usize {
        match access.1 {
            MemoryAccessWidth::MemoryAccess8 => self.ws.access8,
            MemoryAccessWidth::MemoryAccess16 => self.ws.access16,
            MemoryAccessWidth::MemoryAccess32 => self.ws.access32,
        }
    }
}

#[derive(Debug)]
struct DummyBus([u8; 4]);

impl Bus for DummyBus {
    fn read_32(&self, _addr: Addr) -> u32 {
        0
    }

    fn read_16(&self, _addr: Addr) -> u16 {
        0
    }

    fn read_8(&self, _addr: Addr) -> u8 {
        0
    }

    fn write_32(&mut self, _addr: Addr, _value: u32) {}

    fn write_16(&mut self, _addr: Addr, _value: u16) {}

    fn write_8(&mut self, _addr: Addr, _value: u8) {}

    fn get_cycles(&self, _addr: Addr, _access: MemoryAccess) -> usize {
        1
    }
}

#[derive(Debug)]
pub struct SysBus {
    pub io: Rc<RefCell<IoDevices>>,

    bios: BoxedMemory,
    onboard_work_ram: BoxedMemory,
    internal_work_ram: BoxedMemory,
    /// Currently model the IOMem as regular buffer, later make it into something more sophisticated.
    pub ioregs: IoRegs,
    pub palette_ram: BoxedMemory,
    pub vram: BoxedMemory,
    pub oam: BoxedMemory,
    gamepak: Cartridge,
    dummy: DummyBus,
}

impl SysBus {
    pub fn new(
        io: Rc<RefCell<IoDevices>>,
        bios_rom: Vec<u8>,
        gamepak: Cartridge,
        ioregs: IoRegs,
    ) -> SysBus {
        SysBus {
            io: io,

            bios: BoxedMemory::new(bios_rom.into_boxed_slice(), 0xff_ffff),
            onboard_work_ram: BoxedMemory::new_with_waitstate(
                vec![0; WORK_RAM_SIZE].into_boxed_slice(),
                (WORK_RAM_SIZE as u32) - 1,
                WaitState::new(3, 3, 6),
            ),
            internal_work_ram: BoxedMemory::new(
                vec![0; INTERNAL_RAM_SIZE].into_boxed_slice(),
                0x7fff,
            ),
            ioregs: ioregs,
            palette_ram: BoxedMemory::new_with_waitstate(
                vec![0; PALETTE_RAM_SIZE].into_boxed_slice(),
                (PALETTE_RAM_SIZE as u32) - 1,
                WaitState::new(1, 1, 2),
            ),
            vram: BoxedMemory::new_with_waitstate(
                vec![0; VIDEO_RAM_SIZE].into_boxed_slice(),
                (VIDEO_RAM_SIZE as u32) - 1,
                WaitState::new(1, 1, 2),
            ),
            oam: BoxedMemory::new(vec![0; OAM_SIZE].into_boxed_slice(), (OAM_SIZE as u32) - 1),
            gamepak: gamepak,
            dummy: DummyBus([0; 4]),
        }
    }

    fn map(&self, addr: Addr) -> &Bus {
        match (addr & 0xff000000) as usize {
            0x00000000 => &self.bios,
            0x02000000 => &self.onboard_work_ram,
            0x03000000 => &self.internal_work_ram,
            0x04000000 => &self.ioregs,
            0x05000000 => &self.palette_ram,
            0x06000000 => &self.vram,
            0x07000000 => &self.oam,
            0x08000000 => &self.gamepak,
            _ => &self.dummy,
        }
    }

    /// TODO proc-macro for generating this function
    fn map_mut(&mut self, addr: Addr) -> &mut Bus {
        match (addr & 0xff000000) as usize {
            0x00000000 => &mut self.bios,
            0x02000000 => &mut self.onboard_work_ram,
            0x03000000 => &mut self.internal_work_ram,
            0x04000000 => &mut self.ioregs,
            0x05000000 => &mut self.palette_ram,
            0x06000000 => &mut self.vram,
            0x07000000 => &mut self.oam,
            0x08000000 => &mut self.gamepak,
            _ => &mut self.dummy,
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

    fn write_32(&mut self, addr: Addr, value: u32) {
        self.map_mut(addr).write_32(addr & 0xff_ffff, value)
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        self.map_mut(addr).write_16(addr & 0xff_ffff, value)
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        self.map_mut(addr).write_8(addr & 0xff_ffff, value)
    }

    fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize {
        self.map(addr).get_cycles(addr & 0xff_ffff, access)
    }
}
