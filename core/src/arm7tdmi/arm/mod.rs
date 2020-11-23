#[cfg(feature = "debugger")]
pub mod disass;
pub mod exec;

use serde::{Deserialize, Serialize};

use super::alu::*;
use crate::arm7tdmi::{Addr, InstructionDecoder};

use crate::bit::BitIndex;
use crate::byteorder::{LittleEndian, ReadBytesExt};
use crate::num::FromPrimitive;

use std::io;

#[derive(Debug, PartialEq)]
pub enum ArmDecodeErrorKind {
    UnknownInstructionFormat,
    DecodedPartDoesNotBelongToInstruction,
    UndefinedConditionCode(u32),
    InvalidShiftType(u32),
    InvalidHSBits(u32),
    IoError(io::ErrorKind),
}

#[derive(Debug, PartialEq)]
pub struct ArmDecodeError {
    pub kind: ArmDecodeErrorKind,
    pub insn: u32,
    pub addr: Addr,
}

#[allow(dead_code)]
impl ArmDecodeError {
    fn new(kind: ArmDecodeErrorKind, insn: u32, addr: Addr) -> ArmDecodeError {
        ArmDecodeError {
            kind: kind,
            insn: insn,
            addr: addr,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Primitive)]
pub enum ArmCond {
    EQ = 0b0000,
    NE = 0b0001,
    HS = 0b0010,
    LO = 0b0011,
    MI = 0b0100,
    PL = 0b0101,
    VS = 0b0110,
    VC = 0b0111,
    HI = 0b1000,
    LS = 0b1001,
    GE = 0b1010,
    LT = 0b1011,
    GT = 0b1100,
    LE = 0b1101,
    AL = 0b1110,
    Invalid = 0b1111,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq)]
pub enum ArmFormat {
    BranchExchange = 0,
    BranchLink,
    SoftwareInterrupt,
    Multiply,
    MultiplyLong,
    SingleDataTransfer,
    HalfwordDataTransferRegOffset,
    HalfwordDataTransferImmediateOffset,
    DataProcessing,
    BlockDataTransfer,
    SingleDataSwap,
    /// Transfer PSR contents to a register
    MoveFromStatus,
    /// Transfer register contents to PSR
    MoveToStatus,
    /// Tanssfer immediate/register to PSR flags only
    MoveToFlags,

    Undefined,
}

impl From<u32> for ArmFormat {
    fn from(raw: u32) -> ArmFormat {
        use ArmFormat::*;
        if (0x0fff_fff0 & raw) == 0x012f_ff10 {
            BranchExchange
        } else if (0x0e00_0000 & raw) == 0x0a00_0000 {
            BranchLink
        } else if (0xe000_0010 & raw) == 0x0600_0000 {
            Undefined
        } else if (0x0fb0_0ff0 & raw) == 0x0100_0090 {
            SingleDataSwap
        } else if (0x0fc0_00f0 & raw) == 0x0000_0090 {
            Multiply
        } else if (0x0f80_00f0 & raw) == 0x0080_0090 {
            MultiplyLong
        } else if (0x0fbf_0fff & raw) == 0x010f_0000 {
            MoveFromStatus
        } else if (0x0fbf_fff0 & raw) == 0x0129_f000 {
            MoveToStatus
        } else if (0x0dbf_f000 & raw) == 0x0128_f000 {
            MoveToFlags
        } else if (0x0c00_0000 & raw) == 0x0400_0000 {
            SingleDataTransfer
        } else if (0x0e40_0F90 & raw) == 0x0000_0090 {
            HalfwordDataTransferRegOffset
        } else if (0x0e40_0090 & raw) == 0x0040_0090 {
            HalfwordDataTransferImmediateOffset
        } else if (0x0e00_0000 & raw) == 0x0800_0000 {
            BlockDataTransfer
        } else if (0x0f00_0000 & raw) == 0x0f00_0000 {
            SoftwareInterrupt
        } else if (0x0c00_0000 & raw) == 0x0000_0000 {
            DataProcessing
        } else {
            Undefined
        }
    }
}

#[derive(Debug, PartialEq, Primitive)]
pub enum ArmHalfwordTransferType {
    UnsignedHalfwords = 0b01,
    SignedByte = 0b10,
    SignedHalfwords = 0b11,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ArmInstruction {
    pub fmt: ArmFormat,
    pub raw: u32,
    pub pc: Addr,
}

impl ArmInstruction {
    pub fn new(raw: u32, pc: Addr, fmt: ArmFormat) -> ArmInstruction {
        ArmInstruction { fmt, raw, pc }
    }
}

impl InstructionDecoder for ArmInstruction {
    type IntType = u32;

    fn decode(raw: u32, addr: Addr) -> Self {
        let fmt = ArmFormat::from(raw);

        ArmInstruction {
            fmt: fmt,
            raw: raw,
            pc: addr,
        }
    }

    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Self {
        let mut rdr = std::io::Cursor::new(bytes);
        let raw = rdr.read_u32::<LittleEndian>().unwrap();
        Self::decode(raw, addr)
    }

    fn get_raw(&self) -> u32 {
        self.raw
    }
}

pub trait ArmDecodeHelper {
    fn cond(&self) -> ArmCond;

    fn rm(&self) -> usize;

    fn rs(&self) -> usize;

    fn rd_lo(&self) -> usize;

    fn rd_hi(&self) -> usize;

    fn opcode(&self) -> AluOpCode;

    fn branch_offset(&self) -> i32;

    fn load_flag(&self) -> bool;

    fn set_cond_flag(&self) -> bool;

    fn write_back_flag(&self) -> bool;

    fn accumulate_flag(&self) -> bool;

    fn u_flag(&self) -> bool;

    fn halfword_data_transfer_type(&self) -> ArmHalfwordTransferType;

    fn transfer_size(&self) -> usize;

    fn psr_and_force_user_flag(&self) -> bool;

    fn spsr_flag(&self) -> bool;

    fn add_offset_flag(&self) -> bool;

    fn pre_index_flag(&self) -> bool;

    fn link_flag(&self) -> bool;

    /// gets offset used by ldr/str instructions
    fn ldr_str_offset(&self) -> BarrelShifterValue;

    fn get_bs_op(&self) -> BarrelShiftOpCode;

    fn get_shift_reg_by(&self) -> ShiftRegisterBy;

    fn ldr_str_hs_imm_offset(&self) -> BarrelShifterValue;

    fn ldr_str_hs_reg_offset(&self) -> BarrelShifterValue;

    fn operand2(&self) -> BarrelShifterValue;

    fn register_list(&self) -> u16;

    fn swi_comment(&self) -> u32;
}

macro_rules! arm_decode_helper_impl {
    ($($t:ty),*) => {$(

        impl ArmDecodeHelper for $t {
            #[inline(always)]
            fn cond(&self) -> ArmCond {
                ArmCond::from_u32(self.bit_range(28..32)).unwrap()
            }

            #[inline(always)]
            fn rm(&self) -> usize {
                self.bit_range(0..4) as usize
            }

            #[inline(always)]
            fn rs(&self) -> usize {
                self.bit_range(8..12) as usize
            }

            #[inline(always)]
            fn rd_lo(&self) -> usize {
                self.bit_range(12..16) as usize
            }

            #[inline(always)]
            fn rd_hi(&self) -> usize {
                self.bit_range(16..20) as usize
            }

            #[inline(always)]
            fn opcode(&self) -> AluOpCode {
                use std::hint::unreachable_unchecked;

                unsafe {
                    if let Some(opc) = AluOpCode::from_u16(self.bit_range(21..25) as u16) {
                        opc
                    } else {
                        unreachable_unchecked()
                    }
                }
            }

            #[inline(always)]
            fn branch_offset(&self) -> i32 {
                ((self.bit_range(0..24) << 8) as i32) >> 6
            }

            #[inline(always)]
            fn load_flag(&self) -> bool {
                self.bit(20)
            }

            #[inline(always)]
            fn set_cond_flag(&self) -> bool {
                self.bit(20)
            }

            #[inline(always)]
            fn write_back_flag(&self) -> bool {
                self.bit(21)
            }

            #[inline(always)]
            fn accumulate_flag(&self) -> bool {
                self.bit(21)
            }

            #[inline(always)]
            fn u_flag(&self) -> bool {
                self.bit(22)
            }

            #[inline(always)]
            fn halfword_data_transfer_type(&self) -> ArmHalfwordTransferType {
                let bits = (*self & 0b1100000) >> 5;
                ArmHalfwordTransferType::from_u32(bits).unwrap()
            }

            #[inline(always)]
            fn transfer_size(&self) -> usize {
                if self.bit(22) {
                    1
                } else {
                    4
                }
            }

            #[inline(always)]
            fn psr_and_force_user_flag(&self) -> bool {
                self.bit(22)
            }

            #[inline(always)]
            fn spsr_flag(&self) -> bool {
                self.bit(22)
            }

            #[inline(always)]
            fn add_offset_flag(&self) -> bool {
                self.bit(23)
            }

            #[inline(always)]
            fn pre_index_flag(&self) -> bool {
                self.bit(24)
            }

            #[inline(always)]
            fn link_flag(&self) -> bool {
                self.bit(24)
            }

            /// gets offset used by ldr/str instructions
            #[inline(always)]
            fn ldr_str_offset(&self) -> BarrelShifterValue {
                let ofs = self.bit_range(0..12);
                if self.bit(25) {
                    let rm = ofs & 0xf;
                    BarrelShifterValue::ShiftedRegister(ShiftedRegister {
                        reg: rm as usize,
                        shift_by: self.get_shift_reg_by(),
                        bs_op: self.get_bs_op(),
                        added: Some(self.add_offset_flag()),
                    })
                } else {
                    let ofs = if self.add_offset_flag() {
                        ofs as u32
                    } else {
                        -(ofs as i32) as u32
                    };
                    BarrelShifterValue::ImmediateValue(ofs)
                }
            }

            #[inline(always)]
            fn get_bs_op(&self) -> BarrelShiftOpCode {
                BarrelShiftOpCode::from_u8(self.bit_range(5..7) as u8).unwrap()
            }

            #[inline(always)]
            fn get_shift_reg_by(&self) -> ShiftRegisterBy {
                if self.bit(4) {
                    let rs = self.bit_range(8..12) as usize;
                    ShiftRegisterBy::ByRegister(rs)
                } else {
                    let amount = self.bit_range(7..12) as u32;
                    ShiftRegisterBy::ByAmount(amount)
                }
            }

            #[inline(always)]
            fn ldr_str_hs_imm_offset(&self) -> BarrelShifterValue {
                        let offset8 = (self.bit_range(8..12) << 4) + self.bit_range(0..4);
                        let offset8 = if self.add_offset_flag() {
                            offset8
                        } else {
                            (-(offset8 as i32)) as u32
                        };
                        BarrelShifterValue::ImmediateValue(offset8)
            }

            #[inline(always)]
            fn ldr_str_hs_reg_offset(&self) -> BarrelShifterValue  {
                BarrelShifterValue::ShiftedRegister(
                    ShiftedRegister {
                        reg: (self & 0xf) as usize,
                        shift_by: ShiftRegisterBy::ByAmount(0),
                            bs_op: BarrelShiftOpCode::LSL,
                            added: Some(self.add_offset_flag()),
                        })
            }

            fn operand2(&self) -> BarrelShifterValue {
                if self.bit(25) {
                    let immediate = self & 0xff;
                    let rotate = 2 * self.bit_range(8..12);
                    BarrelShifterValue::RotatedImmediate(immediate, rotate)
                } else {
                    let reg = self & 0xf;
                    let shifted_reg = ShiftedRegister {
                        reg: reg as usize,
                        bs_op: self.get_bs_op(),
                        shift_by: self.get_shift_reg_by(),
                        added: None,
                    }; // TODO error handling
                    BarrelShifterValue::ShiftedRegister(shifted_reg)
                }
            }

            fn register_list(&self) -> u16 {
                (self & 0xffff) as u16
            }

            fn swi_comment(&self) -> u32 {
                self.bit_range(0..24)
            }
        }
    )*}

}

arm_decode_helper_impl!(u32);

// #[cfg(test)]
// /// All instructions constants were generated using an ARM assembler.
// mod tests {
//     use super::*;
//     use crate::arm7tdmi::*;
//     use crate::sysbus::BoxedMemory;

//     #[test]
//     fn swi() {
//         let mut core = Core::new();

//         let bytes = vec![];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         // swi #0x1337
//         let decoded = ArmInstruction::decode(0xef001337, 0).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::SoftwareInterrupt);
//         assert_eq!(decoded.swi_comment(), 0x1337);
//         assert_eq!(format!("{}", decoded), "swi\t#0x1337");

//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(core.did_pipeline_flush(), true);

//         assert_eq!(core.cpsr.mode(), CpuMode::Supervisor);
//         assert_eq!(core.pc, Exception::SoftwareInterrupt as u32);
//     }

//     #[test]
//     fn branch_forwards() {
//         // 0x20:   b 0x30
//         let decoded = ArmInstruction::decode(0xea_00_00_02, 0x20).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::BranchLink);
//         assert_eq!(decoded.link_flag(), false);
//         assert_eq!(
//             (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
//             0x30
//         );
//         assert_eq!(format!("{}", decoded), "b\t0x30");

//         let mut core = Core::new();
//         core.pc = 0x20 + 8;

//         let bytes = vec![];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(core.did_pipeline_flush(), true);
//         assert_eq!(core.pc, 0x30);
//     }

//     #[test]
//     fn branch_link_backwards() {
//         // 0x20:   bl 0x10
//         let decoded = ArmInstruction::decode(0xeb_ff_ff_fa, 0x20).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::BranchLink);
//         assert_eq!(decoded.link_flag(), true);
//         assert_eq!(
//             (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
//             0x10
//         );
//         assert_eq!(format!("{}", decoded), "bl\t0x10");

//         let mut core = Core::new();
//         core.pc = 0x20 + 8;

//         let bytes = vec![];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(core.did_pipeline_flush(), true);
//         assert_eq!(core.pc, 0x10);
//     }

//     #[test]
//     fn ldr_pre_index() {
//         // ldreq r2, [r5, -r6, lsl #5]
//         let decoded = ArmInstruction::decode(0x07_15_22_86, 0).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::SingleDataTransfer);
//         assert_eq!(decoded.cond, ArmCond::EQ);
//         assert_eq!(decoded.load_flag(), true);
//         assert_eq!(decoded.pre_index_flag(), true);
//         assert_eq!(decoded.write_back_flag(), false);
//         assert_eq!(decoded.rd(), 2);
//         assert_eq!(decoded.rn(), 5);
//         assert_eq!(
//             decoded.ldr_str_offset(),
//             BarrelShifterValue::ShiftedRegister(ShiftedRegister {
//                 reg: 6,
//                 shift_by: ShiftRegisterBy::ByAmount(5),
//                 bs_op: BarrelShiftOpCode::LSL,
//                 added: Some(false)
//             })
//         );

//         assert_eq!(format!("{}", decoded), "ldreq\tr2, [r5, -r6, lsl #5]");

//         let mut core = Core::new();
//         core.cpsr.set_Z(true);
//         core.gpr[5] = 0x34;
//         core.gpr[6] = 1;
//         core.gpr[2] = 0;

//         #[rustfmt::skip]
//         let bytes = vec![
//             /* 00h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 10h: */ 0x00, 0x00, 0x00, 0x00, 0x37, 0x13, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 20h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 30h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(core.gpr[2], 0x1337);
//     }

//     #[test]
//     fn str_post_index() {
//         // strteq r2, [r4], -r7, asr #8
//         let decoded = ArmInstruction::decode(0x06_24_24_47, 0).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::SingleDataTransfer);
//         assert_eq!(decoded.cond, ArmCond::EQ);
//         assert_eq!(decoded.load_flag(), false);
//         assert_eq!(decoded.pre_index_flag(), false);
//         assert_eq!(decoded.write_back_flag(), true);
//         assert_eq!(decoded.rd(), 2);
//         assert_eq!(decoded.rn(), 4);
//         assert_eq!(
//             decoded.ldr_str_offset(),
//             BarrelShifterValue::ShiftedRegister(ShiftedRegister {
//                 reg: 7,
//                 shift_by: ShiftRegisterBy::ByAmount(8),
//                 bs_op: BarrelShiftOpCode::ASR,
//                 added: Some(false)
//             })
//         );
//         assert_eq!(format!("{}", decoded), "strteq\tr2, [r4], -r7, asr #8");

//         let mut core = Core::new();
//         core.cpsr.set_Z(true);
//         core.gpr[4] = 0x0;
//         core.gpr[7] = 1;
//         core.gpr[2] = 0xabababab;

//         #[rustfmt::skip]
//         let bytes = vec![
//             /* 00h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 10h: */ 0x00, 0x00, 0x00, 0x00, 0x37, 0x13, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 20h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//             /* 30h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(mem.read_32(0), 0xabababab);
//     }

//     #[test]
//     fn str_pre_index() {
//         // str r4, [sp, 0x10]
//         let decoded = ArmInstruction::decode(0xe58d4010, 0).unwrap();
//         assert_eq!(decoded.fmt, ArmFormat::SingleDataTransfer);
//         assert_eq!(decoded.cond, ArmCond::AL);

//         let mut core = Core::new();
//         core.set_reg(4, 0x12345678);
//         core.set_reg(REG_SP, 0);

//         #[rustfmt::skip]
//         let bytes = vec![
//             /*  0: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /*  4: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /*  8: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /*  c: */ 0xaa, 0xbb, 0xcc, 0xdd,
//             /* 10: */ 0xaa, 0xbb, 0xcc, 0xdd,
//         ];
//         let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

//         assert_ne!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
//         core.exec_arm(&mut mem, decoded).unwrap();
//         assert_eq!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
//     }
// }
