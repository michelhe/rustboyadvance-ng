use super::cpu::Core;
use super::Addr;
use std::fmt;

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub enum MemoryAccess {
    NonSeq = 0,
    Seq,
}

impl Default for MemoryAccess {
    fn default() -> MemoryAccess {
        MemoryAccess::NonSeq
    }
}

impl fmt::Display for MemoryAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MemoryAccess::NonSeq => "N",
                MemoryAccess::Seq => "S",
            }
        )
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum MemoryAccessWidth {
    MemoryAccess8 = 0,
    MemoryAccess16,
    MemoryAccess32,
}

/// A trait meant to abstract memory accesses and report the access type back to the user of the arm7tdmi::Core
///
/// struct Memory {
///     data: [u8; 0x4000]
/// }
///
/// impl MemoryInterface for Memory {
///     fn load_8(&mut self, addr: u32, access: MemoryAccess) {
///         debug!("CPU read {:?} cycle", access);
///         self.data[addr & 0x3fff]
///     }
///
///     fn store_8(&mut self, addr: u32, value: u8, access: MemoryAccess) {
///         debug!("CPU write {:?} cycle", access);
///         self.data[addr & 0x3fff] = value;
///     }
///
///     fn idle_cycle(&mut self) {
///         debug!("CPU idle cycle");
///     }
///
///     // implement rest of trait methods
/// }
///
/// let mem = Shared::new(Memory { ... });
/// let cpu = arm7tdmi::Core::new(mem.clone())
///
pub trait MemoryInterface {
    /// Read a byte
    fn load_8(&mut self, addr: u32, access: MemoryAccess) -> u8;
    /// Read a halfword
    fn load_16(&mut self, addr: u32, access: MemoryAccess) -> u16;
    /// Read a word
    fn load_32(&mut self, addr: u32, access: MemoryAccess) -> u32;

    /// Write a byte
    fn store_8(&mut self, addr: u32, value: u8, access: MemoryAccess);
    /// Write a halfword
    fn store_16(&mut self, addr: u32, value: u16, access: MemoryAccess);
    /// Write a word
    fn store_32(&mut self, addr: u32, value: u32, access: MemoryAccess);

    fn idle_cycle(&mut self);
}

impl<I: MemoryInterface> MemoryInterface for Core<I> {
    #[inline]
    fn load_8(&mut self, addr: u32, access: MemoryAccess) -> u8 {
        self.bus.load_8(addr, access)
    }

    #[inline]
    fn load_16(&mut self, addr: u32, access: MemoryAccess) -> u16 {
        self.bus.load_16(addr & !1, access)
    }

    #[inline]
    fn load_32(&mut self, addr: u32, access: MemoryAccess) -> u32 {
        self.bus.load_32(addr & !3, access)
    }

    #[inline]
    fn store_8(&mut self, addr: u32, value: u8, access: MemoryAccess) {
        self.bus.store_8(addr, value, access);
    }
    #[inline]
    fn store_16(&mut self, addr: u32, value: u16, access: MemoryAccess) {
        self.bus.store_16(addr & !1, value, access);
    }

    #[inline]
    fn store_32(&mut self, addr: u32, value: u32, access: MemoryAccess) {
        self.bus.store_32(addr & !3, value, access);
    }

    #[inline]
    fn idle_cycle(&mut self) {
        self.bus.idle_cycle();
    }
}

/// Implementation of memory access helpers
impl<I: MemoryInterface> Core<I> {
    #[inline]
    pub(super) fn store_aligned_32(&mut self, addr: Addr, value: u32, access: MemoryAccess) {
        self.store_32(addr & !0x3, value, access);
    }

    #[inline]
    pub(super) fn store_aligned_16(&mut self, addr: Addr, value: u16, access: MemoryAccess) {
        self.store_16(addr & !0x1, value, access);
    }

    /// Helper function for "ldr" instruction that handles misaligned addresses
    #[inline]
    pub(super) fn ldr_word(&mut self, addr: Addr, access: MemoryAccess) -> u32 {
        if addr & 0x3 != 0 {
            let rotation = (addr & 0x3) << 3;
            let value = self.load_32(addr & !0x3, access);
            let mut carry = self.cpsr.C();
            let v = self.ror(value, rotation, &mut carry, false, false);
            self.cpsr.set_C(carry);
            v
        } else {
            self.load_32(addr, access)
        }
    }

    /// Helper function for "ldrh" instruction that handles misaligned addresses
    #[inline]
    pub(super) fn ldr_half(&mut self, addr: Addr, access: MemoryAccess) -> u32 {
        if addr & 0x1 != 0 {
            let rotation = (addr & 0x1) << 3;
            let value = self.load_16(addr & !0x1, access);
            let mut carry = self.cpsr.C();
            let v = self.ror(value as u32, rotation, &mut carry, false, false);
            self.cpsr.set_C(carry);
            v
        } else {
            self.load_16(addr, access) as u32
        }
    }

    /// Helper function for "ldrsh" instruction that handles misaligned addresses
    #[inline]
    pub(super) fn ldr_sign_half(&mut self, addr: Addr, access: MemoryAccess) -> u32 {
        if addr & 0x1 != 0 {
            self.load_8(addr, access) as i8 as i32 as u32
        } else {
            self.load_16(addr, access) as i16 as i32 as u32
        }
    }
}
