use serde::{Deserialize, Serialize};

use super::arm7tdmi;
use super::arm7tdmi::memory::*;
use super::bios::Bios;
use super::bus::*;
use super::cartridge::Cartridge;
use super::dma::DmaNotifer;
use super::iodev::{IoDevices, WaitControl};
use super::sched::*;
use super::util::{Shared, WeakPointer};

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

// Only the first 15 entries are actually used, the rest are dummy entries for open-bus
const CYCLE_LUT_SIZE: usize = 0x100;

#[derive(Serialize, Deserialize, Clone)]
struct CycleLookupTables {
    n_cycles32: Box<[usize]>,
    s_cycles32: Box<[usize]>,
    n_cycles16: Box<[usize]>,
    s_cycles16: Box<[usize]>,
}

impl Default for CycleLookupTables {
    fn default() -> CycleLookupTables {
        CycleLookupTables {
            n_cycles32: vec![1; CYCLE_LUT_SIZE].into_boxed_slice(),
            s_cycles32: vec![1; CYCLE_LUT_SIZE].into_boxed_slice(),
            n_cycles16: vec![1; CYCLE_LUT_SIZE].into_boxed_slice(),
            s_cycles16: vec![1; CYCLE_LUT_SIZE].into_boxed_slice(),
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

#[derive(Clone)]
pub struct SysBus {
    pub io: Shared<IoDevices>,
    scheduler: Shared<Scheduler>,
    arm_core: WeakPointer<arm7tdmi::Core<SysBus>>,

    bios: Bios,
    ewram: Box<[u8]>,
    iwram: Box<[u8]>,
    pub cartridge: Cartridge,

    cycle_luts: CycleLookupTables,

    pub trace_access: bool,
}

pub type SysBusPtr = WeakPointer<SysBus>;

impl SchedulerConnect for SysBus {
    fn connect_scheduler(&mut self, scheduler: SharedScheduler) {
        self.scheduler = scheduler;
    }
}

impl SysBus {
    pub fn new_with_memories(
        scheduler: Shared<Scheduler>,
        io: Shared<IoDevices>,
        cartridge: Cartridge,
        bios_rom: Box<[u8]>,
        ewram: Box<[u8]>,
        iwram: Box<[u8]>,
    ) -> SysBus {
        let mut luts = CycleLookupTables::default();
        luts.init();
        luts.update_gamepak_waitstates(io.waitcnt);

        SysBus {
            io,
            scheduler,
            arm_core: WeakPointer::default(),
            cartridge,

            bios: Bios::new(bios_rom),
            ewram,
            iwram,
            cycle_luts: luts,
            trace_access: false,
        }
    }

    pub fn new(
        scheduler: Shared<Scheduler>,
        io: Shared<IoDevices>,
        bios_rom: Box<[u8]>,
        cartridge: Cartridge,
    ) -> SysBus {
        let ewram = vec![0; WORK_RAM_SIZE].into_boxed_slice();
        let iwram = vec![0; INTERNAL_RAM_SIZE].into_boxed_slice();
        SysBus::new_with_memories(scheduler, io, cartridge, bios_rom, ewram, iwram)
    }

    pub fn set_ewram(&mut self, buffer: Box<[u8]>) {
        self.ewram = buffer;
    }

    pub fn set_iwram(&mut self, buffer: Box<[u8]>) {
        self.iwram = buffer;
    }

    pub fn get_ewram(&self) -> &[u8] {
        &self.ewram
    }

    pub fn get_iwram(&self) -> &[u8] {
        &self.iwram
    }

    pub fn set_io_devices(&mut self, io_devs: Shared<IoDevices>) {
        self.io = io_devs;
    }

    /// must be called whenever this object is instanciated
    pub fn init(&mut self, arm_core: WeakPointer<arm7tdmi::Core<SysBus>>) {
        self.arm_core = arm_core.clone();
        self.bios.connect_arm_core(arm_core.clone());
        let ptr = SysBusPtr::new(self as *mut SysBus);
        // HACK
        self.io.set_sysbus_ptr(ptr.clone());
    }

    pub fn on_waitcnt_written(&mut self, waitcnt: WaitControl) {
        self.cycle_luts.update_gamepak_waitstates(waitcnt);
    }
    pub fn idle_cycle(&mut self) {
        self.scheduler.update(1);
    }

    #[inline(always)]
    pub fn add_cycles(&mut self, addr: Addr, access: MemoryAccess, width: MemoryAccessWidth) {
        use MemoryAccess::*;
        use MemoryAccessWidth::*;
        let page = ((addr >> 24) & 0xF) as usize;

        let cycles = unsafe {
            match width {
                MemoryAccess8 | MemoryAccess16 => match access {
                    NonSeq => self.cycle_luts.n_cycles16.get_unchecked(page),
                    Seq => self.cycle_luts.s_cycles16.get_unchecked(page),
                },
                MemoryAccess32 => match access {
                    NonSeq => self.cycle_luts.n_cycles32.get_unchecked(page),
                    Seq => self.cycle_luts.s_cycles32.get_unchecked(page),
                },
            }
        };

        self.scheduler.update(*cycles);
    }

    /// Helper for "open-bus" accesses
    /// http://problemkaputt.de/gbatek.htm#gbaunpredictablethings
    /// Reading from Unused Memory (00004000-01FFFFFF,10000000-FFFFFFFF)
    /// `addr` is considered to be an address of
    fn read_invalid(&mut self, addr: Addr) -> u32 {
        warn!("invalid read @{:08x}", addr);
        use super::arm7tdmi::CpuState;
        let value = match self.arm_core.cpsr.state() {
            CpuState::ARM => self.arm_core.get_prefetched_opcode(),
            CpuState::THUMB => {
                // For THUMB code the result consists of two 16bit fragments and depends on the address area
                // and alignment where the opcode was stored.

                let decoded = self.arm_core.get_decoded_opcode() & 0xffff; // [$+2]
                let prefetched = self.arm_core.get_prefetched_opcode() & 0xffff; // [$+4]
                let r15 = self.arm_core.pc;
                let mut value = prefetched;
                match (r15 >> 24) as usize {
                    PAGE_BIOS | PAGE_OAM => {
                        // TODO this is probably wrong, according to GBATEK, we should be using $+6 here but it isn't prefetched yet.
                        value = value << 16;
                        value |= decoded;
                    }
                    PAGE_IWRAM => {
                        // OldLO=[$+2], OldHI=[$+2]
                        if r15 & 3 == 0 {
                            // LSW = [$+4], MSW = OldHI   ;for opcodes at 4-byte aligned locations
                            value |= decoded << 16;
                        } else {
                            // LSW = OldLO, MSW = [$+4]   ;for opcodes at non-4-byte aligned locations
                            value = value << 16;
                            value |= decoded;
                        }
                    }
                    _ => value |= value << 16,
                }
                value
            }
        };
        value >> ((addr & 3) << 3)
    }
}

/// Todo - implement bound checks for EWRAM/IWRAM
impl Bus for SysBus {
    #[inline]
    fn read_32(&mut self, addr: Addr) -> u32 {
        match addr & 0xff000000 {
            BIOS_ADDR => {
                if addr <= 0x3ffc {
                    self.bios.read_32(addr)
                } else {
                    self.read_invalid(addr)
                }
            }
            EWRAM_ADDR => self.ewram.read_32(addr & 0x3_fffc),
            IWRAM_ADDR => self.iwram.read_32(addr & 0x7ffc),
            IOMEM_ADDR => {
                let addr = if addr & 0xfffc == 0x8000 {
                    0x800
                } else {
                    addr & 0x00fffffc
                };
                self.io.read_32(addr)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.read_32(addr),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI | GAMEPAK_WS1_LO | GAMEPAK_WS1_HI | GAMEPAK_WS2_LO => {
                self.cartridge.read_32(addr)
            }
            GAMEPAK_WS2_HI => self.cartridge.read_32(addr),
            SRAM_LO | SRAM_HI => self.cartridge.read_32(addr),
            _ => self.read_invalid(addr),
        }
    }

    #[inline]
    fn read_16(&mut self, addr: Addr) -> u16 {
        match addr & 0xff000000 {
            BIOS_ADDR => {
                if addr <= 0x3ffe {
                    self.bios.read_16(addr)
                } else {
                    self.read_invalid(addr) as u16
                }
            }
            EWRAM_ADDR => self.ewram.read_16(addr & 0x3_fffe),
            IWRAM_ADDR => self.iwram.read_16(addr & 0x7ffe),
            IOMEM_ADDR => {
                let addr = if addr & 0xfffe == 0x8000 {
                    0x800
                } else {
                    addr & 0x00fffffe
                };
                self.io.read_16(addr)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.read_16(addr),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI | GAMEPAK_WS1_LO | GAMEPAK_WS1_HI | GAMEPAK_WS2_LO => {
                self.cartridge.read_16(addr)
            }
            GAMEPAK_WS2_HI => self.cartridge.read_16(addr),
            SRAM_LO | SRAM_HI => self.cartridge.read_16(addr),
            _ => self.read_invalid(addr) as u16,
        }
    }

    #[inline]
    fn read_8(&mut self, addr: Addr) -> u8 {
        match addr & 0xff000000 {
            BIOS_ADDR => {
                if addr <= 0x3fff {
                    self.bios.read_8(addr)
                } else {
                    self.read_invalid(addr) as u8
                }
            }
            EWRAM_ADDR => self.ewram.read_8(addr & 0x3_ffff),
            IWRAM_ADDR => self.iwram.read_8(addr & 0x7fff),
            IOMEM_ADDR => {
                let addr = if addr & 0xffff == 0x8000 {
                    0x800
                } else {
                    addr & 0x00ffffff
                };
                self.io.read_8(addr)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.read_8(addr),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI | GAMEPAK_WS1_LO | GAMEPAK_WS1_HI | GAMEPAK_WS2_LO => {
                self.cartridge.read_8(addr)
            }
            GAMEPAK_WS2_HI => self.cartridge.read_8(addr),
            SRAM_LO | SRAM_HI => self.cartridge.read_8(addr),
            _ => self.read_invalid(addr) as u8,
        }
    }

    #[inline]
    fn write_32(&mut self, addr: Addr, value: u32) {
        match addr & 0xff000000 {
            BIOS_ADDR => {}
            EWRAM_ADDR => self.ewram.write_32(addr & 0x3_fffc, value),
            IWRAM_ADDR => self.iwram.write_32(addr & 0x7ffc, value),
            IOMEM_ADDR => {
                let addr = if addr & 0xfffc == 0x8000 {
                    0x800
                } else {
                    addr & 0x00fffffc
                };
                self.io.write_32(addr, value)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.write_32(addr, value),
            GAMEPAK_WS0_LO => self.cartridge.write_32(addr, value),
            GAMEPAK_WS2_HI => self.cartridge.write_32(addr, value),
            SRAM_LO | SRAM_HI => self.cartridge.write_32(addr, value),
            _ => {
                // warn!("trying to write invalid address {:#x}", addr);
                // TODO open bus
            }
        }
    }

    #[inline]
    fn write_16(&mut self, addr: Addr, value: u16) {
        match addr & 0xff000000 {
            BIOS_ADDR => {}
            EWRAM_ADDR => self.ewram.write_16(addr & 0x3_fffe, value),
            IWRAM_ADDR => self.iwram.write_16(addr & 0x7ffe, value),
            IOMEM_ADDR => {
                let addr = if addr & 0xfffe == 0x8000 {
                    0x800
                } else {
                    addr & 0x00fffffe
                };
                self.io.write_16(addr, value)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.write_16(addr, value),
            GAMEPAK_WS0_LO => self.cartridge.write_16(addr, value),
            GAMEPAK_WS2_HI => self.cartridge.write_16(addr, value),
            SRAM_LO | SRAM_HI => self.cartridge.write_16(addr, value),
            _ => {
                // warn!("trying to write invalid address {:#x}", addr);
                // TODO open bus
            }
        }
    }

    #[inline]
    fn write_8(&mut self, addr: Addr, value: u8) {
        match addr & 0xff000000 {
            BIOS_ADDR => {}
            EWRAM_ADDR => self.ewram.write_8(addr & 0x3_ffff, value),
            IWRAM_ADDR => self.iwram.write_8(addr & 0x7fff, value),
            IOMEM_ADDR => {
                let addr = if addr & 0xffff == 0x8000 {
                    0x800
                } else {
                    addr & 0x00ffffff
                };
                self.io.write_8(addr, value)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.write_8(addr, value),
            GAMEPAK_WS0_LO => self.cartridge.write_8(addr, value),
            GAMEPAK_WS2_HI => self.cartridge.write_8(addr, value),
            SRAM_LO | SRAM_HI => self.cartridge.write_8(addr, value),
            _ => {
                // warn!("trying to write invalid address {:#x}", addr);
                // TODO open bus
            }
        }
    }
}

impl DebugRead for SysBus {
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        match addr & 0xff000000 {
            BIOS_ADDR => self.bios.debug_read_8(addr),
            EWRAM_ADDR => self.ewram.debug_read_8(addr & 0x3_ffff),
            IWRAM_ADDR => self.iwram.debug_read_8(addr & 0x7fff),
            IOMEM_ADDR => {
                let addr = if addr & 0xffff == 0x8000 {
                    0x800
                } else {
                    addr & 0x00ffffff
                };
                self.io.debug_read_8(addr)
            }
            PALRAM_ADDR | VRAM_ADDR | OAM_ADDR => self.io.gpu.debug_read_8(addr),
            GAMEPAK_WS0_LO | GAMEPAK_WS0_HI | GAMEPAK_WS1_LO | GAMEPAK_WS1_HI | GAMEPAK_WS2_LO => {
                self.cartridge.debug_read_8(addr)
            }
            GAMEPAK_WS2_HI => self.cartridge.debug_read_8(addr),
            SRAM_LO | SRAM_HI => self.cartridge.debug_read_8(addr),
            _ => {
                // TODO open-bus
                0
            }
        }
    }
}

impl MemoryInterface for SysBus {
    #[inline]
    fn load_8(&mut self, addr: u32, access: MemoryAccess) -> u8 {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess8);
        self.read_8(addr)
    }

    #[inline]
    fn load_16(&mut self, addr: u32, access: MemoryAccess) -> u16 {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess16);
        self.read_16(addr)
    }

    #[inline]
    fn load_32(&mut self, addr: u32, access: MemoryAccess) -> u32 {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess32);
        self.read_32(addr)
    }

    #[inline]
    fn store_8(&mut self, addr: u32, value: u8, access: MemoryAccess) {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess8);
        self.write_8(addr, value);
    }

    #[inline]
    fn store_16(&mut self, addr: u32, value: u16, access: MemoryAccess) {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess8);
        self.write_16(addr, value);
    }

    #[inline]
    fn store_32(&mut self, addr: u32, value: u32, access: MemoryAccess) {
        self.add_cycles(addr, access, MemoryAccessWidth::MemoryAccess8);
        self.write_32(addr, value);
    }

    #[inline]
    fn idle_cycle(&mut self) {
        self.scheduler.update(1)
    }
}

impl DmaNotifer for SysBus {
    fn notify(&mut self, timing: u16) {
        self.io.dmac.notify_from_gpu(timing);
    }
}
