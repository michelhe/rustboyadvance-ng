use super::arm7tdmi;
use super::bus::{Addr, Bus, DebugRead};
use super::util::WeakPointer;
use super::SysBus;

/// Struct representing the sytem ROM
#[derive(Clone)]
pub struct Bios {
    /// Underlying memory
    rom: Box<[u8]>,
    /// Last read value
    last_opcode: u32,
    /// Arm pointer - used only to read the PC register
    arm_core: WeakPointer<arm7tdmi::Core<SysBus>>,
}

impl Bios {
    pub fn new(bios_rom: Box<[u8]>) -> Bios {
        Bios {
            rom: bios_rom,
            last_opcode: 0xe129f000, // the opcode at [00DCh+8]
            arm_core: WeakPointer::default(),
        }
    }

    pub(super) fn connect_arm_core(&mut self, arm_ptr: WeakPointer<arm7tdmi::Core<SysBus>>) {
        self.arm_core = arm_ptr;
    }

    #[inline]
    fn read_allowed(&self) -> bool {
        self.arm_core.pc < 0x4000
    }
}

/// Impl of Bus trait for Bios
impl Bus for Bios {
    #[inline]
    fn read_32(&mut self, addr: Addr) -> u32 {
        if self.read_allowed() {
            let value = self.rom.read_32(addr);
            // 32-bit read from bios is most probably an opcode fetch
            self.last_opcode = value;
            value
        } else {
            self.last_opcode
        }
    }
    #[inline]
    fn read_16(&mut self, addr: Addr) -> u16 {
        if self.read_allowed() {
            self.rom.read_16(addr) as u16
        } else {
            (self.last_opcode >> ((addr & 2) << 3)) as u16
        }
    }

    #[inline]
    fn read_8(&mut self, addr: Addr) -> u8 {
        if self.read_allowed() {
            self.rom.read_8(addr)
        } else {
            (self.last_opcode >> ((addr & 3) << 3)) as u8
        }
    }

    #[inline]
    fn write_8(&mut self, _addr: Addr, _value: u8) {
        // The bios is RO
    }
}

impl DebugRead for Bios {
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        self.rom[addr as usize]
    }
}
