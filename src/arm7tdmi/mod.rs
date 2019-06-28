use std::fmt;

use crate::num_traits::FromPrimitive;

pub mod arm;
use arm::*;

pub mod cpu;
mod exception;
mod psr;

pub use super::sysbus;

pub const REG_PC: usize = 15;
pub const REG_LR: usize = 14;
pub const REG_SP: usize = 13;

pub fn reg_string(reg: usize) -> &'static str {
    let reg_names = &[
        "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp", "lr",
        "pc",
    ];
    reg_names[reg]
}

#[derive(Debug, PartialEq)]
pub enum CpuInstruction {
    Arm(ArmInstruction),
    Thumb,
}

#[derive(Debug, PartialEq, Primitive, Copy, Clone)]
#[repr(u8)]
pub enum CpuState {
    ARM = 0,
    THUMB = 1,
}

impl fmt::Display for CpuState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use CpuState::*;
        match self {
            ARM => write!(f, "ARM"),
            THUMB => write!(f, "THUMB"),
        }
    }
}

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
pub enum CpuMode {
    User = 0b10000,
    Fiq = 0b10001,
    Irq = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    Undefined = 0b11011,
    System = 0b11111,
}

impl CpuMode {
    pub fn spsr_index(&self) -> Option<usize> {
        match self {
            CpuMode::Fiq => Some(0),
            CpuMode::Irq => Some(1),
            CpuMode::Supervisor => Some(2),
            CpuMode::Abort => Some(3),
            CpuMode::Undefined => Some(4),
            _ => None,
        }
    }

    pub fn bank_index(&self) -> usize {
        match self {
            CpuMode::User | CpuMode::System => 0,
            CpuMode::Fiq => 1,
            CpuMode::Irq => 2,
            CpuMode::Supervisor => 3,
            CpuMode::Abort => 4,
            CpuMode::Undefined => 5,
        }
    }
}

impl fmt::Display for CpuMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use CpuMode::*;
        match self {
            User => write!(f, "USR"),
            Fiq => write!(f, "FIQ"),
            Irq => write!(f, "IRQ"),
            Supervisor => write!(f, "SVC"),
            Abort => write!(f, "ABT"),
            Undefined => write!(f, "UND"),
            System => write!(f, "SYS"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CpuError {
    ArmDecodeError(ArmDecodeError),
    IllegalInstruction,
    UnimplementedCpuInstruction(CpuInstruction),
}

impl From<ArmDecodeError> for CpuError {
    fn from(e: ArmDecodeError) -> CpuError {
        CpuError::ArmDecodeError(e)
    }
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CpuError::ArmDecodeError(e) => write!(
                f,
                "arm decoding error at address @0x{:08x} (instruction 0x{:08x}): {:?}",
                e.addr, e.insn, e.kind
            ),
            CpuError::UnimplementedCpuInstruction(CpuInstruction::Arm(insn)) => write!(
                f,
                "unimplemented instruction: 0x{:08x}:\t0x{:08x}\t{}",
                insn.pc, insn.raw, insn
            ),
            CpuError::IllegalInstruction => write!(f, "illegal instruction"),
            e => write!(f, "error: {:#x?}", e),
        }
    }
}

pub type CpuResult<T> = Result<T, CpuError>;

pub struct CpuModeContext {
    // r8-r14
    banked_gpr: [u32; 7],
    spsr: u32,
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
