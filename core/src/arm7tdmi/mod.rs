use std::fmt;

use num::Num;
use serde::{Deserialize, Serialize};

pub mod arm;
pub mod thumb;

use arm::ArmInstruction;
use thumb::ThumbInstruction;

pub mod cpu;
pub use cpu::*;
pub mod alu;
pub mod memory;
pub use alu::*;
pub mod exception;
pub mod psr;
pub use psr::*;
pub mod disass;

pub const REG_PC: usize = 15;
pub const REG_LR: usize = 14;
pub const REG_SP: usize = 13;

pub(self) use crate::Addr;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DecodedInstruction {
    Arm(ArmInstruction),
    Thumb(ThumbInstruction),
}

impl DecodedInstruction {
    pub fn get_pc(&self) -> Addr {
        match self {
            DecodedInstruction::Arm(a) => a.pc,
            DecodedInstruction::Thumb(t) => t.pc,
        }
    }
}

#[cfg(feature = "debugger")]
impl fmt::Display for DecodedInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodedInstruction::Arm(a) => write!(f, "{}", a),
            DecodedInstruction::Thumb(t) => write!(f, "{}", t),
        }
    }
}

pub trait InstructionDecoder: Sized {
    type IntType: Num;

    fn decode(n: Self::IntType, addr: Addr) -> Self;
    /// Helper functions for the Disassembler
    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Self;
    fn get_raw(&self) -> Self::IntType;
}

pub fn reg_string<T: Into<usize>>(reg: T) -> &'static str {
    let reg_names = &[
        "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp", "lr",
        "pc",
    ];
    reg_names[reg.into()]
}

#[derive(Debug, PartialEq, Primitive, Copy, Clone)]
#[repr(u8)]
pub enum CpuState {
    ARM = 0,
    THUMB = 1,
}

impl fmt::Display for CpuState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
