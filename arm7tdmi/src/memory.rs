use super::Arm7tdmiCore;
use std::fmt;

pub type Addr = u32;

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

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum MemoryAccessWidth {
    MemoryAccess8 = 0,
    MemoryAccess16,
    MemoryAccess32,
}

/// A trait meant to abstract memory accesses and report the access type back to the user of the arm7tdmi::Arm7tdmiCore
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
/// let cpu = arm7tdmi::Arm7tdmiCore::new(mem.clone())
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

impl<I: MemoryInterface> MemoryInterface for Arm7tdmiCore<I> {
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
impl<I: MemoryInterface> Arm7tdmiCore<I> {
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

/// Simple trait for accessing bus peripherals (higher level API than the low-level MemoryInterface)
pub trait BusIO {
    fn read_32(&mut self, addr: Addr) -> u32 {
        self.read_16(addr) as u32 | (self.read_16(addr + 2) as u32) << 16
    }

    fn read_16(&mut self, addr: Addr) -> u16 {
        self.default_read_16(addr)
    }

    #[inline(always)]
    fn default_read_16(&mut self, addr: Addr) -> u16 {
        self.read_8(addr) as u16 | (self.read_8(addr + 1) as u16) << 8
    }

    fn read_8(&mut self, addr: Addr) -> u8;

    fn write_32(&mut self, addr: Addr, value: u32) {
        self.write_16(addr, (value & 0xffff) as u16);
        self.write_16(addr + 2, (value >> 16) as u16);
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        self.default_write_16(addr, value)
    }

    #[inline(always)]
    fn default_write_16(&mut self, addr: Addr, value: u16) {
        self.write_8(addr, (value & 0xff) as u8);
        self.write_8(addr + 1, ((value >> 8) & 0xff) as u8);
    }

    fn write_8(&mut self, addr: Addr, value: u8);

    fn get_bytes(&mut self, range: std::ops::Range<u32>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.read_8(b));
        }
        bytes
    }
}

/// Helper trait for reading memory as if we were an all-powerfull debugger
pub trait DebugRead: BusIO {
    fn debug_read_32(&mut self, addr: Addr) -> u32 {
        self.debug_read_16(addr) as u32 | (self.debug_read_16(addr + 2) as u32) << 16
    }

    fn debug_read_16(&mut self, addr: Addr) -> u16 {
        self.debug_read_8(addr) as u16 | (self.debug_read_8(addr + 1) as u16) << 8
    }

    fn debug_read_8(&mut self, addr: Addr) -> u8;

    fn debug_get_bytes(&mut self, range: std::ops::Range<Addr>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in range {
            bytes.push(self.debug_read_8(b));
        }
        bytes
    }

    fn debug_get_into_bytes(&mut self, start_addr: Addr, bytes: &mut [u8]) {
        bytes
            .iter_mut()
            .enumerate()
            .for_each(|(idx, byte)| *byte = self.debug_read_8(start_addr + (idx as Addr)));
    }
}

/// The caller is assumed to handle out of bound accesses,
/// For performance reasons, this impl trusts that 'addr' is within the array range.
impl BusIO for Box<[u8]> {
    #[inline]
    fn read_8(&mut self, addr: Addr) -> u8 {
        unsafe { *self.get_unchecked(addr as usize) }
    }

    #[inline]
    fn write_8(&mut self, addr: Addr, value: u8) {
        unsafe {
            *self.get_unchecked_mut(addr as usize) = value;
        }
    }
}

impl DebugRead for Box<[u8]> {
    #[inline]
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        self[addr as usize]
    }
}
