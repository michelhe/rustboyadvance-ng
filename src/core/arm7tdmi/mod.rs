use std::fmt;

use num::Num;
use serde::{Deserialize, Serialize};

pub mod arm;
pub mod thumb;

use arm::{ArmDecodeError, ArmInstruction};
use thumb::{ThumbDecodeError, ThumbInstruction};

pub mod cpu;
pub use cpu::*;
pub mod alu;
pub use alu::*;
pub mod exception;
pub mod psr;

pub const REG_PC: usize = 15;
pub const REG_LR: usize = 14;
pub const REG_SP: usize = 13;

pub(self) use crate::core::{Addr, Bus};

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
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
impl fmt::Display for DecodedInstruction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodedInstruction::Arm(a) => write!(f, "{}", a),
            DecodedInstruction::Thumb(t) => write!(f, "{}", t),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum InstructionDecoderError {
    ArmDecodeError(ArmDecodeError),
    ThumbDecodeError(ThumbDecodeError),
    IoError(std::io::ErrorKind),
}

impl From<ArmDecodeError> for InstructionDecoderError {
    fn from(e: ArmDecodeError) -> InstructionDecoderError {
        InstructionDecoderError::ArmDecodeError(e)
    }
}

impl From<ThumbDecodeError> for InstructionDecoderError {
    fn from(e: ThumbDecodeError) -> InstructionDecoderError {
        InstructionDecoderError::ThumbDecodeError(e)
    }
}

pub trait InstructionDecoder: Sized + fmt::Display {
    type IntType: Num;

    fn decode(n: Self::IntType, addr: Addr) -> Result<Self, InstructionDecoderError>;
    /// Helper functions for the Disassembler
    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Result<Self, InstructionDecoderError>;
    fn get_raw(&self) -> Self::IntType;
}

pub fn reg_string(reg: usize) -> &'static str {
    let reg_names = &[
        "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp", "lr",
        "pc",
    ];
    reg_names[reg]
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
    DecodeError(InstructionDecoderError),
    IllegalInstruction,
    UnimplementedCpuInstruction(Addr, u32, DecodedInstruction),
}

impl From<InstructionDecoderError> for CpuError {
    fn from(e: InstructionDecoderError) -> CpuError {
        CpuError::DecodeError(e)
    }
}

impl From<ArmDecodeError> for CpuError {
    fn from(e: ArmDecodeError) -> CpuError {
        CpuError::DecodeError(InstructionDecoderError::ArmDecodeError(e))
    }
}

impl From<ThumbDecodeError> for CpuError {
    fn from(e: ThumbDecodeError) -> CpuError {
        CpuError::DecodeError(InstructionDecoderError::ThumbDecodeError(e))
    }
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CpuError::DecodeError(InstructionDecoderError::ArmDecodeError(e)) => write!(
                f,
                "arm decoding error at address @0x{:08x} (instruction 0x{:08x}): {:?}",
                e.addr, e.insn, e.kind
            ),
            CpuError::DecodeError(InstructionDecoderError::ThumbDecodeError(e)) => write!(
                f,
                "thumb decoding error at address @0x{:08x} (instruction 0x{:08x}): {:?}",
                e.addr, e.insn, e.kind
            ),
            CpuError::UnimplementedCpuInstruction(addr, raw, d) => write!(
                f,
                "unimplemented instruction: 0x{:08x}:\t0x{:08x}\t{:?}",
                addr, raw, d
            ),
            CpuError::IllegalInstruction => write!(f, "illegal instruction"),
            e => write!(f, "error: {:#x?}", e),
        }
    }
}

pub type CpuResult<T> = Result<T, CpuError>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
