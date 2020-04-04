use std::fmt;
use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};

use super::cartridge::Cartridge;
use super::gpu::VIDEO_RAM_SIZE;
use super::iodev::{IoDevices, WaitControl};
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

    pub const PAGE_BIOS: usize = (BIOS_ADDR >> 24) as usize;
    pub const PAGE_EWRAM: usize = (EWRAM_ADDR >> 24) as usize;
    pub const PAGE_IWRAM: usize = (IWRAM_ADDR >> 24) as usize;
    pub const PAGE_IOMEM: usize = (IOMEM_ADDR >> 24) as usize;
    pub const PAGE_PALRAM: usize = (PALRAM_ADDR >> 24) as usize;
    pub const PAGE_VRAM: usize = (VRAM_ADDR >> 24) as usize;
    pub const PAGE_OAM: usize = (OAM_ADDR >> 24) as usize;
    pub const PAGE_GAMEPAK_WS0: usize = (GAMEPAK_WS0_LO >> 24) as usize;
    pub const PAGE_GAMEPAK_WS1: usize = (GAMEPAK_WS1_LO >> 24) as usize;
    pub const PAGE_GAMEPAK_WS2: usize = (GAMEPAK_WS2_LO >> 24) as usize;
    pub const PAGE_SRAM_LO: usize = (SRAM_LO >> 24) as usize;
    pub const PAGE_SRAM_HI: usize = (SRAM_HI >> 24) as usize;
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

const CYCLE_LUT_SIZE: usize = 0x10;

#[derive(Serialize, Deserialize, Clone)]
struct CycleLookupTables {
    n_cycles32: [usize; CYCLE_LUT_SIZE],
    s_cycles32: [usize; CYCLE_LUT_SIZE],
    n_cycles16: [usize; CYCLE_LUT_SIZE],
    s_cycles16: [usize; CYCLE_LUT_SIZE],
}

impl Default for CycleLookupTables {
    fn default() -> CycleLookupTables {
        CycleLookupTables {
            n_cycles32: [1; CYCLE_LUT_SIZE],
            s_cycles32: [1; CYCLE_LUT_SIZE],
            n_cycles16: [1; CYCLE_LUT_SIZE],
            s_cycles16: [1; CYCLE_LUT_SIZE],
        }
    }
}

impl CycleLookupTables {
    pub fn init(&mut self) {
        self.n_cycles32[PAGE_EWRAM] = 6;
        self.s_cycles32[PAGE_EWRAM] = 6;
        self.n_cycles16[PAGE_EWRAM] = 3;
        self.s_cycles16[PAGE_EWRAM] = 3;

        self.n_cycles32[PAGE_OAM] = 2;
        self.s_cycles32[PAGE_OAM] = 2;
        self.n_cycles16[PAGE_OAM] = 1;
        self.s_cycles16[PAGE_OAM] = 1;

        self.n_cycles32[PAGE_VRAM] = 2;
        self.s_cycles32[PAGE_VRAM] = 2;
        self.n_cycles16[PAGE_VRAM] = 1;
        self.s_cycles16[PAGE_VRAM] = 1;

        self.n_cycles32[PAGE_PALRAM] = 2;
        self.s_cycles32[PAGE_PALRAM] = 2;
        self.n_cycles16[PAGE_PALRAM] = 1;
        self.s_cycles16[PAGE_PALRAM] = 1;
    }

    pub fn update_gamepak_waitstates(&mut self, waitcnt: WaitControl) {
        static S_GAMEPAK_NSEQ_CYCLES: [usize; 4] = [4, 3, 2, 8];
        static S_GAMEPAK_WS0_SEQ_CYCLES: [usize; 2] = [2, 1];
        static S_GAMEPAK_WS1_SEQ_CYCLES: [usize; 2] = [4, 1];
        static S_GAMEPAK_WS2_SEQ_CYCLES: [usize; 2] = [8, 1];

        let ws0_first_access = waitcnt.ws0_first_access() as usize;
        let ws1_first_access = waitcnt.ws1_first_access() as usize;
        let ws2_first_access = waitcnt.ws2_first_access() as usize;
        let ws0_second_access = waitcnt.ws0_second_access() as usize;
        let ws1_second_access = waitcnt.ws1_second_access() as usize;
        let ws2_second_access = waitcnt.ws2_second_access() as usize;

        // update SRAM access
        let sram_wait_cycles = 1 + S_GAMEPAK_NSEQ_CYCLES[waitcnt.sram_wait_control() as usize];
        self.n_cycles32[PAGE_SRAM_LO] = sram_wait_cycles;
        self.n_cycles32[PAGE_SRAM_LO] = sram_wait_cycles;
        self.n_cycles16[PAGE_SRAM_HI] = sram_wait_cycles;
        self.n_cycles16[PAGE_SRAM_HI] = sram_wait_cycles;
        self.s_cycles32[PAGE_SRAM_LO] = sram_wait_cycles;
        self.s_cycles32[PAGE_SRAM_LO] = sram_wait_cycles;
        self.s_cycles16[PAGE_SRAM_HI] = sram_wait_cycles;
        self.s_cycles16[PAGE_SRAM_HI] = sram_wait_cycles;

        // update both pages of each waitstate
        for i in 0..2 {
            self.n_cycles16[PAGE_GAMEPAK_WS0 + i] = 1 + S_GAMEPAK_NSEQ_CYCLES[ws0_first_access];
            self.s_cycles16[PAGE_GAMEPAK_WS0 + i] = 1 + S_GAMEPAK_WS0_SEQ_CYCLES[ws0_second_access];

            self.n_cycles16[PAGE_GAMEPAK_WS1 + i] = 1 + S_GAMEPAK_NSEQ_CYCLES[ws1_first_access];
            self.s_cycles16[PAGE_GAMEPAK_WS1 + i] = 1 + S_GAMEPAK_WS1_SEQ_CYCLES[ws1_second_access];

            self.n_cycles16[PAGE_GAMEPAK_WS2 + i] = 1 + S_GAMEPAK_NSEQ_CYCLES[ws2_first_access];
            self.s_cycles16[PAGE_GAMEPAK_WS2 + i] = 1 + S_GAMEPAK_WS2_SEQ_CYCLES[ws2_second_access];

            // ROM 32bit accesses are split into two 16bit accesses 1N+1S
            self.n_cycles32[PAGE_GAMEPAK_WS0 + i] =
                self.n_cycles16[PAGE_GAMEPAK_WS0 + i] + self.s_cycles16[PAGE_GAMEPAK_WS0 + i];
            self.n_cycles32[PAGE_GAMEPAK_WS1 + i] =
                self.n_cycles16[PAGE_GAMEPAK_WS1 + i] + self.s_cycles16[PAGE_GAMEPAK_WS1 + i];
            self.n_cycles32[PAGE_GAMEPAK_WS2 + i] =
                self.n_cycles16[PAGE_GAMEPAK_WS2 + i] + self.s_cycles16[PAGE_GAMEPAK_WS2 + i];

            self.s_cycles32[PAGE_GAMEPAK_WS0 + i] = 2 * self.s_cycles16[PAGE_GAMEPAK_WS0 + i];
            self.s_cycles32[PAGE_GAMEPAK_WS1 + i] = 2 * self.s_cycles16[PAGE_GAMEPAK_WS1 + i];
            self.s_cycles32[PAGE_GAMEPAK_WS2 + i] = 2 * self.s_cycles16[PAGE_GAMEPAK_WS2 + i];
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SysBus {
    pub io: IoDevices,

    bios: BoxedMemory,
    onboard_work_ram: BoxedMemory,
    internal_work_ram: BoxedMemory,
    pub cartridge: Cartridge,
    dummy: DummyBus,

    cycle_luts: CycleLookupTables,

    pub trace_access: bool,
}

#[repr(transparent)]
#[derive(Clone)]
pub struct SysBusPtr {
    ptr: *mut SysBus,
}

impl Default for SysBusPtr {
    fn default() -> SysBusPtr {
        SysBusPtr {
            ptr: std::ptr::null_mut::<SysBus>(),
        }
    }
}

impl SysBusPtr {
    pub fn new(ptr: *mut SysBus) -> SysBusPtr {
        SysBusPtr { ptr: ptr }
    }
}

impl Deref for SysBusPtr {
    type Target = SysBus;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for SysBusPtr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

impl SysBus {
    pub fn new(io: IoDevices, bios_rom: Box<[u8]>, cartridge: Cartridge) -> SysBus {
        let mut luts = CycleLookupTables::default();
        luts.init();
        luts.update_gamepak_waitstates(io.waitcnt);

        SysBus {
            io: io,

            bios: BoxedMemory::new(bios_rom),
            onboard_work_ram: BoxedMemory::new(vec![0; WORK_RAM_SIZE].into_boxed_slice()),
            internal_work_ram: BoxedMemory::new(vec![0; INTERNAL_RAM_SIZE].into_boxed_slice()),
            cartridge: cartridge,
            dummy: DummyBus([0; 4]),

            cycle_luts: luts,

            trace_access: false,
        }
    }

    /// must be called whenever this object is instanciated
    pub fn created(&mut self) {
        let ptr = SysBusPtr::new(self as *mut SysBus);
        // HACK
        self.io.set_sysbus_ptr(ptr.clone());
    }

    pub fn on_waitcnt_written(&mut self, waitcnt: WaitControl) {
        self.cycle_luts.update_gamepak_waitstates(waitcnt);
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

    #[inline(always)]
    pub fn get_cycles(
        &self,
        addr: Addr,
        access: MemoryAccessType,
        width: MemoryAccessWidth,
    ) -> usize {
        use MemoryAccessType::*;
        use MemoryAccessWidth::*;
        let page = (addr >> 24) as usize;

        // TODO optimize out by making the LUTs have 0x100 entries for each possible page ?
        if page > 0xF {
            // open bus
            return 1;
        }
        match width {
            MemoryAccess8 | MemoryAccess16 => match access {
                NonSeq => self.cycle_luts.n_cycles16[page],
                Seq => self.cycle_luts.s_cycles16[page],
            },
            MemoryAccess32 => match access {
                NonSeq => self.cycle_luts.n_cycles32[page],
                Seq => self.cycle_luts.s_cycles32[page],
            },
        }
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
