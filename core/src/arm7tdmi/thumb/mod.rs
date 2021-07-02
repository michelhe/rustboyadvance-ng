use super::alu::*;
use super::arm::*;
use super::{Addr, InstructionDecoder};
use crate::bit::BitIndex;
use crate::byteorder::{LittleEndian, ReadBytesExt};
use crate::num::FromPrimitive;

#[cfg(feature = "debugger")]
pub mod disass;
pub mod exec;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq)]
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
    LoadAddress,
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

    /// Not an actual thumb format
    Undefined,
}

impl From<u16> for ThumbFormat {
    fn from(raw: u16) -> ThumbFormat {
        use ThumbFormat::*;
        if raw & 0xf800 == 0x1800 {
            AddSub
        } else if raw & 0xe000 == 0x0000 {
            MoveShiftedReg
        } else if raw & 0xe000 == 0x2000 {
            DataProcessImm
        } else if raw & 0xfc00 == 0x4000 {
            AluOps
        } else if raw & 0xfc00 == 0x4400 {
            HiRegOpOrBranchExchange
        } else if raw & 0xf800 == 0x4800 {
            LdrPc
        } else if raw & 0xf200 == 0x5000 {
            LdrStrRegOffset
        } else if raw & 0xf200 == 0x5200 {
            LdrStrSHB
        } else if raw & 0xe000 == 0x6000 {
            LdrStrImmOffset
        } else if raw & 0xf000 == 0x8000 {
            LdrStrHalfWord
        } else if raw & 0xf000 == 0x9000 {
            LdrStrSp
        } else if raw & 0xf000 == 0xa000 {
            LoadAddress
        } else if raw & 0xff00 == 0xb000 {
            AddSp
        } else if raw & 0xf600 == 0xb400 {
            PushPop
        } else if raw & 0xf000 == 0xc000 {
            LdmStm
        } else if raw & 0xff00 == 0xdf00 {
            Swi
        } else if raw & 0xf000 == 0xd000 {
            BranchConditional
        } else if raw & 0xf800 == 0xe000 {
            Branch
        } else if raw & 0xf000 == 0xf000 {
            BranchLongWithLink
        } else {
            Undefined
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ThumbInstruction {
    pub fmt: ThumbFormat,
    pub raw: u16,
    pub pc: Addr,
}

impl ThumbInstruction {
    pub fn new(raw: u16, pc: Addr, fmt: ThumbFormat) -> ThumbInstruction {
        ThumbInstruction { fmt, raw, pc }
    }
}

impl InstructionDecoder for ThumbInstruction {
    type IntType = u16;

    fn decode(raw: u16, addr: Addr) -> Self {
        let fmt = ThumbFormat::from(raw);
        ThumbInstruction::new(raw, addr, fmt)
    }

    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Self {
        let mut rdr = std::io::Cursor::new(bytes);
        let raw = rdr.read_u16::<LittleEndian>().unwrap();
        Self::decode(raw, addr)
    }

    fn get_raw(&self) -> u16 {
        self.raw
    }
}

#[derive(Debug, Primitive, PartialEq)]
pub enum OpFormat3 {
    MOV = 0,
    CMP = 1,
    ADD = 2,
    SUB = 3,
}

impl From<OpFormat3> for AluOpCode {
    fn from(op: OpFormat3) -> AluOpCode {
        match op {
            OpFormat3::MOV => AluOpCode::MOV,
            OpFormat3::CMP => AluOpCode::CMP,
            OpFormat3::ADD => AluOpCode::ADD,
            OpFormat3::SUB => AluOpCode::SUB,
        }
    }
}

#[derive(Debug, Primitive, PartialEq)]
pub enum OpFormat5 {
    ADD = 0,
    CMP = 1,
    MOV = 2,
    BX = 3,
}

#[derive(Debug, Primitive, PartialEq)]
pub enum ThumbAluOps {
    AND = 0b0000,
    EOR = 0b0001,
    LSL = 0b0010,
    LSR = 0b0011,
    ASR = 0b0100,
    ADC = 0b0101,
    SBC = 0b0110,
    ROR = 0b0111,
    TST = 0b1000,
    NEG = 0b1001,
    CMP = 0b1010,
    CMN = 0b1011,
    ORR = 0b1100,
    MUL = 0b1101,
    BIC = 0b1110,
    MVN = 0b1111,
}

impl ThumbAluOps {
    pub fn is_setting_flags(&self) -> bool {
        use ThumbAluOps::*;
        match self {
            TST | CMP | CMN => true,
            _ => false,
        }
    }
    pub fn is_arithmetic(&self) -> bool {
        use ThumbAluOps::*;
        match self {
            ADC | SBC | NEG | CMP | CMN => true,
            _ => false,
        }
    }
}

impl From<OpFormat5> for AluOpCode {
    fn from(op: OpFormat5) -> AluOpCode {
        match op {
            OpFormat5::ADD => AluOpCode::ADD,
            OpFormat5::CMP => AluOpCode::CMP,
            OpFormat5::MOV => AluOpCode::MOV,
            OpFormat5::BX => panic!("this should not be called if op = BX"),
        }
    }
}

/// A trait which provides methods to extract thumb instruction fields
pub trait ThumbDecodeHelper {
    // Consts

    // Methods

    fn rs(&self) -> usize;

    fn rb(&self) -> usize;

    fn ro(&self) -> usize;

    fn rn(&self) -> usize;

    fn format1_op(&self) -> BarrelShiftOpCode;

    fn format3_op(&self) -> OpFormat3;

    fn format5_op(&self) -> OpFormat5;

    fn format4_alu_op(&self) -> ThumbAluOps;

    fn offset5(&self) -> u8;

    fn bcond_offset(&self) -> i32;

    fn offset11(&self) -> i32;

    fn word8(&self) -> u16;

    fn is_load(&self) -> bool;

    fn is_subtract(&self) -> bool;

    fn is_immediate_operand(&self) -> bool;

    fn cond(&self) -> ArmCond;

    fn flag(self, bit: usize) -> bool;

    fn register_list(&self) -> u8;

    fn sword7(&self) -> i32;
}

macro_rules! thumb_decode_helper_impl {
    ($($t:ty),*) => {$(

        impl ThumbDecodeHelper for $t {

            #[inline]
            fn rs(&self) -> usize {
                self.bit_range(3..6) as usize
            }

            #[inline]
            /// Note: not true for LdmStm
            fn rb(&self) -> usize {
                self.bit_range(3..6) as usize
            }

            #[inline]
            fn ro(&self) -> usize {
                self.bit_range(6..9) as usize
            }

            #[inline]
            fn rn(&self) -> usize {
                self.bit_range(6..9) as usize
            }

            #[inline]
            fn format1_op(&self) -> BarrelShiftOpCode {
                BarrelShiftOpCode::from_u8(self.bit_range(11..13) as u8).unwrap()
            }

            #[inline]
            fn format3_op(&self) -> OpFormat3 {
                OpFormat3::from_u8(self.bit_range(11..13) as u8).unwrap()
            }

            #[inline]
            fn format5_op(&self) -> OpFormat5 {
                OpFormat5::from_u8(self.bit_range(8..10) as u8).unwrap()
            }

            #[inline]
            fn format4_alu_op(&self) -> ThumbAluOps {
                ThumbAluOps::from_u16(self.bit_range(6..10)).unwrap()
            }

            #[inline]
            fn offset5(&self) -> u8 {
                self.bit_range(6..11) as u8
            }

            #[inline]
            fn bcond_offset(&self) -> i32 {
                ((((*self & 0xff) as u32) << 24) as i32) >> 23
            }

            #[inline]
            fn offset11(&self) -> i32 {
                (*self & 0x7FF) as i32
            }

            #[inline]
            fn word8(&self) -> u16 {
                (*self & 0xff) << 2
            }

            #[inline]
            fn is_load(&self) -> bool {
                self.bit(11)
            }

            #[inline]
            fn is_subtract(&self) -> bool {
                self.bit(9)
            }

            #[inline]
            fn is_immediate_operand(&self) -> bool {
                self.bit(10)
            }

            #[inline]
            fn cond(&self) -> ArmCond {
                ArmCond::from_u8(self.bit_range(8..12) as u8).expect("bad condition")
            }

            #[inline]
            fn flag(self, bit: usize) -> bool {
                self.bit(bit)
            }

            #[inline]
            fn register_list(&self) -> u8 {
                (*self & 0xff) as u8
            }

            #[inline]
            fn sword7(&self) -> i32 {
                let imm7 = *self & 0x7f;
                if self.bit(7) {
                    -((imm7 << 2) as i32)
                } else {
                    (imm7 << 2) as i32
                }
            }
        }

    )*}
}

thumb_decode_helper_impl!(u16);

// #[cfg(test)]
// /// All instructions constants were generated using an ARM assembler.
// mod tests {
//     use super::super::Core;
//     use super::*;
//     use crate::sysbus::BoxedMemory;
//     use crate::Bus;

//     #[test]
//     fn mov_low_reg() {
//         let bytes = vec![];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);
//         let mut core = Core::new();
//         core.set_reg(0, 0);

//         // movs r0, #0x27
//         let insn = ThumbInstruction::decode(0x2027, 0).unwrap();

//         assert_eq!(format!("{}", insn), "mov\tr0, #0x27");
//         core.exec_thumb(&mut mem, insn).unwrap();
//         assert_eq!(core.get_reg(0), 0x27);
//     }

//     // #[test]
//     // fn decode_add_sub() {
//     //     let insn = ThumbInstruction::decode(0xac19, 0).unwrap();
//     //     assert!(format!("add\tr4, r4"))
//     // }

//     #[test]
//     fn ldr_pc() {
//         // ldr r0, [pc, #4]
//         let insn = ThumbInstruction::decode(0x4801, 0x6).unwrap();

//         #[rustfmt::skip]
//         let bytes = vec![
//             /* 0: */ 0x00, 0x00,
//             /* 2: */ 0x00, 0x00,
//             /* 4: */ 0x00, 0x00,
//             /* 6: <pc> */ 0x00, 0x00,
//             /* 8: */ 0x00, 0x00, 0x00, 0x00,
//             /* c: */ 0x78, 0x56, 0x34, 0x12,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);
//         let mut core = Core::new();
//         core.set_reg(0, 0);

//         assert_eq!(format!("{}", insn), "ldr\tr0, [pc, #0x4] ; = #0xc");
//         core.exec_thumb(&mut mem, insn).unwrap();
//         assert_eq!(core.get_reg(0), 0x12345678);
//     }

//     #[test]
//     fn ldr_str_reg_offset() {
//         // str	r0, [r4, r1]
//         let str_insn = ThumbInstruction::decode(0x5060, 0x6).unwrap();
//         // ldrb r2, [r4, r1]
//         let ldr_insn = ThumbInstruction::decode(0x5c62, 0x6).unwrap();

//         let mut core = Core::new();
//         core.set_reg(0, 0x12345678);
//         core.set_reg(2, 0);
//         core.set_reg(1, 0x4);
//         core.set_reg(4, 0xc);

//         #[rustfmt::skip]
//         let bytes = vec![
//             /* 00h: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 04h: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 08h: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 0ch: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 10h: */ 0xaa, 0xbb, 0xcc, 0xdd,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         assert_eq!(format!("{}", str_insn), "str\tr0, [r4, r1]");
//         assert_eq!(format!("{}", ldr_insn), "ldrb\tr2, [r4, r1]");
//         core.exec_thumb(&mut mem, str_insn).unwrap();
//         assert_eq!(mem.read_32(0x10), 0x12345678);
//         core.exec_thumb(&mut mem, ldr_insn).unwrap();
//         assert_eq!(core.get_reg(2), 0x78);
//     }

//     #[allow(overflowing_literals)]
//     #[test]
//     fn format8() {
//         let mut core = Core::new();
//         #[rustfmt::skip]
//         let bytes = vec![
//             /* 00h: */ 0xaa, 0xbb, 0xcc, 0xdd, 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 08h: */ 0xaa, 0xbb, 0xcc, 0xdd, 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 10h: */ 0xaa, 0xbb, 0xcc, 0xdd, 0xaa, 0xbb, 0xcc, 0xdd,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         core.gpr[4] = 0x12345678;
//         core.gpr[3] = 0x2;
//         core.gpr[0] = 0x4;
//         // strh r4, [r3, r0]
//         let decoded = ThumbInstruction::decode(0x521c, 0).unwrap();
//         assert_eq!(format!("{}", decoded), "strh\tr4, [r3, r0]");
//         core.exec_thumb(&mut mem, decoded).unwrap();
//         assert_eq!(&mem.get_bytes(0x6)[..4], [0x78, 0x56, 0xaa, 0xbb]);

//         // ldsb r2, [r7, r1]
//         core.gpr[2] = 0;
//         core.gpr[7] = 0x10;
//         core.gpr[1] = 0x5;
//         let decoded = ThumbInstruction::decode(0x567a, 0).unwrap();
//         assert_eq!(format!("{}", decoded), "ldsb\tr2, [r7, r1]");
//         core.exec_thumb(&mut mem, decoded).unwrap();
//         assert_eq!(core.gpr[2], mem.read_8(0x15) as i8 as u32);

//         // ldsh r3, [r4, r2]
//         core.gpr[3] = 0x0;
//         core.gpr[4] = 0x0;
//         core.gpr[2] = 0x6;
//         let decoded = ThumbInstruction::decode(0x5ea3, 0).unwrap();
//         assert_eq!(format!("{}", decoded), "ldsh\tr3, [r4, r2]");
//         core.exec_thumb(&mut mem, decoded).unwrap();
//         assert_eq!(core.gpr[3], 0x5678);
//     }
// }
