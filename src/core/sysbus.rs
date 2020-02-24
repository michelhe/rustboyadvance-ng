use std::fmt;
use std::ops::Add;

use serde::{Deserialize, Serialize};

use super::cartridge::Cartridge;
use super::gpu::{GpuState, VIDEO_RAM_SIZE};
use super::iodev::IoDevices;
use super::{Addr, Bus};

pub mod consts {
    pub const WORK_RAM_SIZE: usize = 256 * 1024;
    pub const INTERNAL_RAM_SIZE: usize = 32 * 1024;

    pub const BIOS_ADDR: u32 = 0x0000_0000;
    pub const EWRAM_ADDR: u32 = 0x0200_0000;
    pub const IWRAM_ADDR: u32 = 0x0300_0000;
    pub const IOMEM_ADDR: u32 = 0x0400_0000;
    pub const PALRAM_ADDR: u32 = 0x0500_0000;
    pub const VRAM_ADDR: u32 = 0x0600_0000;
    pub const OAM_ADDR: u32 = 0x0700_0000;
    pub const GAMEPAK_WS0_LO: u32 = 0x0800_0000;
    pub const GAMEPAK_WS0_HI: u32 = 0x0900_0000;
    pub const GAMEPAK_WS1_LO: u32 = 0x0A00_0000;
    pub const GAMEPAK_WS1_HI: u32 = 0x0B00_0000;
    pub const GAMEPAK_WS2_LO: u32 = 0x0C00_0000;
    pub const GAMEPAK_WS2_HI: u32 = 0x0D00_0000;
    pub const SRAM_LO: u32 = 0x0E00_0000;
    pub const SRAM_HI: u32 = 0x0F00_0000;
}

use consts::*;

#[derive(Debug, Copy, Clone)]
pub enum MemoryAccessType {
    NonSeq,
    Seq,
}

impl fmt::Display for MemoryAccessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

#[derive(Debug, PartialEq, Copy, Clone)]
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

#[derive(Debug, Copy, Clone)]
pub struct MemoryAccess(pub MemoryAccessType, pub MemoryAccessWidth);

impl fmt::Display for MemoryAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-Cycle ({:?})", self.0, self.1)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[repr(transparent)]
pub struct BoxedMemory {
    pub mem: Box<[u8]>,
}

impl BoxedMemory {
    pub fn new(boxed_slice: Box<[u8]>) -> BoxedMemory {
        BoxedMemory { mem: boxed_slice }
    }
}

impl Bus for BoxedMemory {
    fn read_8(&self, addr: Addr) -> u8 {
        unsafe { *self.mem.get_unchecked(addr as usize) }
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        unsafe {
            *self.mem.get_unchecked_mut(addr as usize) = value;
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct DummyBus([u8; 4]);

impl Bus for DummyBus {
    fn read_8(&self, _addr: Addr) -> u8 {
        0
    }

    fn write_8(&mut self, _addr: Addr, _value: u8) {}
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SysBus {
    pub io: IoDevices,

    bios: BoxedMemory,
    onboard_work_ram: BoxedMemory,
    internal_work_ram: BoxedMemory,
    pub cartridge: Cartridge,
    dummy: DummyBus,

    pub trace_access: bool,
}

impl SysBus {
    pub fn new(io: IoDevices, bios_rom: Vec<u8>, cartridge: Cartridge) -> SysBus {
        SysBus {
            io: io,

            bios: BoxedMemory::new(bios_rom.into_boxed_slice()),
            onboard_work_ram: BoxedMemory::new(vec![0; WORK_RAM_SIZE].into_boxed_slice()),
            internal_work_ram: BoxedMemory::new(vec![0; INTERNAL_RAM_SIZE].into_boxed_slice()),
            cartridge: cartridge,
            dummy: DummyBus([0; 4]),

            trace_access: false,
        }
    }

    fn map(&self, addr: Addr) -> (&dyn Bus, Addr) {
        match addr & 0xff000000 {
            BIOS_ADDR => {
                if addr >= 0x4000 {
                    (&self.dummy, addr) // TODO return last fetched opcode
                } else {
                    (&self.bios, addr)
                }
            }
            EWRAM_ADDR => (&self.onboard_work_ram, addr & 0x3_ffff),
            IWRAM_ADDR => (&self.internal_work_ram, addr & 0x7fff),
            IOMEM_ADDR => (&self.io, {
                if addr & 0xffff == 0x8000 {
                    0x800
                } else {
                    addr & 0x7ff
                }
            }),
            PALRAM_ADDR => (&self.io.gpu.palette_ram, addr & 0x3ff),
            VRAM_ADDR => (&self.io.gpu.vram, {
                let mut ofs = addr & ((VIDEO_RAM_SIZE as u32) - 1);
                if ofs > 0x18000 {
                    ofs -= 0x8000;
                }
                ofs
            }),
            OAM_ADDR => (&self.io.gpu.oam, addr & 0x3ff),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI | GAMEPAK_WS1_LO | GAMEPAK_WS1_HI | GAMEPAK_WS2_LO => {
                (&self.cartridge, addr)
            }
            GAMEPAK_WS2_HI => (&self.cartridge, addr),
            SRAM_LO | SRAM_HI => (&self.cartridge, addr),
            _ => {
                warn!("trying to read invalid address {:#x}", addr);
                (&self.dummy, addr)
            }
        }
    }

    /// TODO proc-macro for generating this function
    fn map_mut(&mut self, addr: Addr) -> (&mut dyn Bus, Addr) {
        match addr & 0xff000000 {
            BIOS_ADDR => (&mut self.dummy, addr),
            EWRAM_ADDR => (&mut self.onboard_work_ram, addr & 0x3_ffff),
            IWRAM_ADDR => (&mut self.internal_work_ram, addr & 0x7fff),
            IOMEM_ADDR => (&mut self.io, {
                if addr & 0xffff == 0x8000 {
                    0x800
                } else {
                    addr & 0x7ff
                }
            }),
            PALRAM_ADDR => (&mut self.io.gpu.palette_ram, addr & 0x3ff),
            VRAM_ADDR => (&mut self.io.gpu.vram, {
                let mut ofs = addr & ((VIDEO_RAM_SIZE as u32) - 1);
                if ofs > 0x18000 {
                    ofs -= 0x8000;
                }
                ofs
            }),
            OAM_ADDR => (&mut self.io.gpu.oam, addr & 0x3ff),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI => (&mut self.dummy, addr),
            GAMEPAK_WS2_HI => (&mut self.cartridge, addr),
            SRAM_LO | SRAM_HI => (&mut self.cartridge, addr),
            _ => {
                warn!("trying to write invalid address {:#x}", addr);
                (&mut self.dummy, addr)
            }
        }
    }

    pub fn get_cycles(&self, addr: Addr, access: MemoryAccess) -> usize {
        let nonseq_cycles = [4, 3, 2, 8];
        let seq_cycles = [2, 1];

        let mut cycles = 0;

        // TODO handle EWRAM accesses
        match addr & 0xff000000 {
            EWRAM_ADDR => match access.1 {
                MemoryAccessWidth::MemoryAccess32 => cycles += 6,
                _ => cycles += 3,
            },
            OAM_ADDR | VRAM_ADDR | PALRAM_ADDR => {
                match access.1 {
                    MemoryAccessWidth::MemoryAccess32 => cycles += 2,
                    _ => cycles += 1,
                }
                if self.io.gpu.state == GpuState::HDraw {
                    cycles += 1;
                }
            }
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI => match access.0 {
                MemoryAccessType::NonSeq => match access.1 {
                    MemoryAccessWidth::MemoryAccess32 => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws0_first_access() as usize];
                        cycles += seq_cycles[self.io.waitcnt.ws0_second_access() as usize];
                    }
                    _ => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws0_first_access() as usize];
                    }
                },
                MemoryAccessType::Seq => {
                    cycles += seq_cycles[self.io.waitcnt.ws0_second_access() as usize];
                    if access.1 == MemoryAccessWidth::MemoryAccess32 {
                        cycles += seq_cycles[self.io.waitcnt.ws0_second_access() as usize];
                    }
                }
            },
            GAMEPAK_WS1_LO | GAMEPAK_WS1_HI => match access.0 {
                MemoryAccessType::NonSeq => match access.1 {
                    MemoryAccessWidth::MemoryAccess32 => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws1_first_access() as usize];
                        cycles += seq_cycles[self.io.waitcnt.ws1_second_access() as usize];
                    }
                    _ => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws1_first_access() as usize];
                    }
                },
                MemoryAccessType::Seq => {
                    cycles += seq_cycles[self.io.waitcnt.ws1_second_access() as usize];
                    if access.1 == MemoryAccessWidth::MemoryAccess32 {
                        cycles += seq_cycles[self.io.waitcnt.ws1_second_access() as usize];
                    }
                }
            },
            GAMEPAK_WS2_LO | GAMEPAK_WS2_HI => match access.0 {
                MemoryAccessType::NonSeq => match access.1 {
                    MemoryAccessWidth::MemoryAccess32 => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws2_first_access() as usize];
                        cycles += seq_cycles[self.io.waitcnt.ws2_second_access() as usize];
                    }
                    _ => {
                        cycles += nonseq_cycles[self.io.waitcnt.ws2_first_access() as usize];
                    }
                },
                MemoryAccessType::Seq => {
                    cycles += seq_cycles[self.io.waitcnt.ws2_second_access() as usize];
                    if access.1 == MemoryAccessWidth::MemoryAccess32 {
                        cycles += seq_cycles[self.io.waitcnt.ws2_second_access() as usize];
                    }
                }
            },
            _ => {}
        }

        cycles
    }
}

impl Bus for SysBus {
    fn read_32(&self, addr: Addr) -> u32 {
        if addr & 3 != 0 {
            warn!("Unaligned read32 at {:#X}", addr);
        }
        let (dev, addr) = self.map(addr & !3);
        dev.read_32(addr)
    }

    fn read_16(&self, addr: Addr) -> u16 {
        if addr & 1 != 0 {
            warn!("Unaligned read16 at {:#X}", addr);
        }
        let (dev, addr) = self.map(addr & !1);
        dev.read_16(addr)
    }

    fn read_8(&self, addr: Addr) -> u8 {
        let (dev, addr) = self.map(addr);
        dev.read_8(addr)
    }

    fn write_32(&mut self, addr: Addr, value: u32) {
        if addr & 3 != 0 {
            warn!("Unaligned write32 at {:#X} (value={:#X}", addr, value);
        }
        let (dev, addr) = self.map_mut(addr & !3);
        dev.write_32(addr, value);
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        if addr & 1 != 0 {
            warn!("Unaligned write16 at {:#X} (value={:#X}", addr, value);
        }
        let (dev, addr) = self.map_mut(addr & !1);
        dev.write_16(addr, value);
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        let (dev, addr) = self.map_mut(addr);
        dev.write_8(addr, value);
    }
}
