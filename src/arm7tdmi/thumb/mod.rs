use std::io;

use crate::bit::BitIndex;
use crate::byteorder::{LittleEndian, ReadBytesExt};
use crate::num::FromPrimitive;

use super::arm::{ArmCond, ArmShiftType};
use super::{Addr, InstructionDecoder, InstructionDecoderError};

pub mod display;
pub mod exec;

#[derive(Debug, PartialEq)]
pub enum ThumbDecodeErrorKind {
    UnknownInstructionFormat,
    IoError(io::ErrorKind),
}
use ThumbDecodeErrorKind::*;

#[derive(Debug, PartialEq)]
pub struct ThumbDecodeError {
    pub kind: ThumbDecodeErrorKind,
    pub insn: u16,
    pub addr: Addr,
}

impl ThumbDecodeError {
    fn new(kind: ThumbDecodeErrorKind, insn: u16, addr: Addr) -> ThumbDecodeError {
        ThumbDecodeError {
            kind: kind,
            insn: insn,
            addr: addr,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ThumbFormat {
    /// Format 1
    MoveShiftedReg,
    /// Format 2
    AddSub,
    /// Format 3
    DataProcessImm,
    /// Format 4
    AluOps,
    /// Format 5
    HiRegOpOrBranchExchange,
    /// Format 6
    LdrPc,
    /// Format 7
    LdrStrRegOffset,
    /// Format 8
    LdrStrSHB,
    /// Format 9
    LdrStrImmOffset,
    /// Format 10
    LdrStrHalfWord,
    /// Format 11
    LdrStrSp,
    /// Format 12
    LdrAddress,
    /// Format 13
    AddSp,
    /// Format 14
    PushPop,
    /// Format 15
    LdmStm,
    /// Format 16
    BranchConditional,
    /// Format 17
    Swi,
    /// Format 18
    Branch,
    /// Format 19
    BranchLongWithLink,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ThumbInstruction {
    pub fmt: ThumbFormat,
    pub raw: u16,
    pub pc: Addr,
}

impl InstructionDecoder for ThumbInstruction {
    type IntType = u16;

    fn decode(raw: u16, addr: Addr) -> Result<Self, InstructionDecoderError> {
        use self::ThumbFormat::*;

        let fmt = if raw & 0xf800 == 0x1800 {
            Ok(AddSub)
        } else if raw & 0xe000 == 0x0000 {
            Ok(MoveShiftedReg)
        } else if raw & 0xe000 == 0x2000 {
            Ok(DataProcessImm)
        } else if raw & 0xfc00 == 0x4000 {
            Ok(AluOps)
        } else if raw & 0xfc00 == 0x4400 {
            Ok(HiRegOpOrBranchExchange)
        } else if raw & 0xf800 == 0x4800 {
            Ok(LdrPc)
        } else if raw & 0xf200 == 0x5000 {
            Ok(LdrStrRegOffset)
        } else if raw & 0xf200 == 0x5200 {
            Ok(LdrStrSHB)
        } else if raw & 0xe000 == 0x6000 {
            Ok(LdrStrImmOffset)
        } else if raw & 0xf000 == 0x8000 {
            Ok(LdrStrHalfWord)
        } else if raw & 0xf000 == 0x9000 {
            Ok(LdrStrSp)
        } else if raw & 0xf000 == 0xa000 {
            Ok(LdrAddress)
        } else if raw & 0xff00 == 0xb000 {
            Ok(AddSp)
        } else if raw & 0xf600 == 0xb400 {
            Ok(PushPop)
        } else if raw & 0xf000 == 0xc000 {
            Ok(LdmStm)
        } else if raw & 0xf000 == 0xd000 {
            Ok(BranchConditional)
        } else if raw & 0xff00 == 0xdf00 {
            Ok(Swi)
        } else if raw & 0xf800 == 0xe000 {
            Ok(Branch)
        } else if raw & 0xf000 == 0xf000 {
            Ok(BranchLongWithLink)
        } else {
            Err(ThumbDecodeError::new(UnknownInstructionFormat, raw, addr))
        }?;

        Ok(ThumbInstruction {
            fmt: fmt,
            raw: raw,
            pc: addr,
        })
    }

    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Result<Self, InstructionDecoderError> {
        let mut rdr = std::io::Cursor::new(bytes);
        let raw = rdr
            .read_u16::<LittleEndian>()
            .map_err(|e| InstructionDecoderError::IoError(e.kind()))?;
        Self::decode(raw, addr)
    }

    fn get_raw(&self) -> u16 {
        self.raw
    }
}

#[derive(Debug, Primitive)]
pub enum OpFormat3 {
    MOV = 0,
    CMP = 1,
    ADD = 2,
    SUB = 3,
}

#[derive(Debug, Primitive)]
pub enum OpFormat5 {
    ADD = 0,
    CMP = 1,
    MOV = 2,
    BX = 3,
}

impl ThumbInstruction {
    const FLAG_H1: usize = 7;
    const FLAG_H2: usize = 6;

    pub fn rd(&self) -> usize {
        (self.raw & 0b111) as usize
    }

    pub fn rs(&self) -> usize {
        self.raw.bit_range(3..6) as usize
    }

    pub fn rb(&self) -> usize {
        self.raw.bit_range(3..6) as usize
    }

    pub fn ro(&self) -> usize {
        self.raw.bit_range(6..9) as usize
    }

    pub fn rn(&self) -> usize {
        self.raw.bit_range(6..9) as usize
    }

    pub fn format1_op(&self) -> ArmShiftType {
        ArmShiftType::from_u8(self.raw.bit_range(11..13) as u8).unwrap()
    }

    pub fn format3_op(&self) -> OpFormat3 {
        OpFormat3::from_u8(self.raw.bit_range(11..13) as u8).unwrap()
    }

    pub fn format5_op(&self) -> OpFormat5 {
        OpFormat5::from_u8(self.raw.bit_range(8..10) as u8).unwrap()
    }

    pub fn offset5(&self) -> i8 {
        self.raw.bit_range(6..11) as i8
    }

    pub fn offset8(&self) -> i8 {
        self.raw.bit_range(0..8) as i8
    }

    pub fn word8(&self) -> u16 {
        self.raw.bit_range(0..8) << 2
    }

    pub fn is_transfering_bytes(&self) -> bool {
        self.raw.bit(10)
    }

    pub fn is_load(&self) -> bool {
        self.raw.bit(11)
    }

    pub fn is_subtract(&self) -> bool {
        self.raw.bit(9)
    }

    pub fn is_immediate_operand(&self) -> bool {
        self.raw.bit(10)
    }

    pub fn cond(&self) -> ArmCond {
        ArmCond::from_u8(self.raw.bit_range(8..12) as u8).unwrap()
    }

    pub fn flag(&self, bit: usize) -> bool {
        self.raw.bit(bit)
    }
}
