pub mod display;
pub mod exec;

use crate::arm7tdmi::{Addr, InstructionDecoder, InstructionDecoderError};

use crate::bit::BitIndex;
use crate::byteorder::{LittleEndian, ReadBytesExt};
use crate::num::FromPrimitive;

use std::convert::TryFrom;
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
use ArmDecodeErrorKind::*;

#[derive(Debug, PartialEq)]
pub struct ArmDecodeError {
    pub kind: ArmDecodeErrorKind,
    pub insn: u32,
    pub addr: Addr,
}

impl ArmDecodeError {
    fn new(kind: ArmDecodeErrorKind, insn: u32, addr: Addr) -> ArmDecodeError {
        ArmDecodeError {
            kind: kind,
            insn: insn,
            addr: addr,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Primitive)]
pub enum ArmCond {
    Equal = 0b0000,
    NotEqual = 0b0001,
    UnsignedHigherOrSame = 0b0010,
    UnsignedLower = 0b0011,
    Negative = 0b0100,
    PositiveOrZero = 0b0101,
    Overflow = 0b0110,
    NoOverflow = 0b0111,
    UnsignedHigher = 0b1000,
    UnsignedLowerOrSame = 0b1001,
    GreaterOrEqual = 0b1010,
    LessThan = 0b1011,
    GreaterThan = 0b1100,
    LessThanOrEqual = 0b1101,
    Always = 0b1110,
}

#[derive(Debug, Copy, Clone, PartialEq)]
#[allow(non_camel_case_types)]
pub enum ArmFormat {
    /// Branch and Exchange
    BX,
    /// Branch /w Link
    B_BL,
    /// Software interrupt
    SWI,
    // Multiply and Multiply-Accumulate
    MUL_MLA,
    /// Multiply Long and Multiply-Accumulate Long
    MULL_MLAL,
    /// Single Data Transfer
    LDR_STR,
    /// Halfword and Signed Data Transfer
    LDR_STR_HS_REG,
    /// Halfword and Signed Data Transfer
    LDR_STR_HS_IMM,
    /// Data Processing
    DP,
    /// Block Data Transfer
    LDM_STM,
    /// Single Data Swap
    SWP,
    /// Transfer PSR contents to a register
    MRS,
    /// Transfer register contents to PSR
    MSR_REG,
    /// Tanssfer immediate/register to PSR flags only
    MSR_FLAGS,
}

#[derive(Debug, Primitive)]
pub enum ArmOpCode {
    AND = 0b0000,
    EOR = 0b0001,
    SUB = 0b0010,
    RSB = 0b0011,
    ADD = 0b0100,
    ADC = 0b0101,
    SBC = 0b0110,
    RSC = 0b0111,
    TST = 0b1000,
    TEQ = 0b1001,
    CMP = 0b1010,
    CMN = 0b1011,
    ORR = 0b1100,
    MOV = 0b1101,
    BIC = 0b1110,
    MVN = 0b1111,
}

impl ArmOpCode {
    pub fn is_setting_flags(&self) -> bool {
        match self {
            ArmOpCode::TST | ArmOpCode::TEQ | ArmOpCode::CMP | ArmOpCode::CMN => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Primitive)]
pub enum ArmHalfwordTransferType {
    UnsignedHalfwords = 0b01,
    SignedByte = 0b10,
    SignedHalfwords = 0b11,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmInstruction {
    pub cond: ArmCond,
    pub fmt: ArmFormat,
    pub raw: u32,
    pub pc: Addr,
}

impl InstructionDecoder for ArmInstruction {
    type IntType = u32;

    fn decode(raw: u32, addr: Addr) -> Result<Self, InstructionDecoderError> {
        use ArmFormat::*;
        let cond_code = raw.bit_range(28..32) as u8;
        let cond = match ArmCond::from_u8(cond_code) {
            Some(cond) => Ok(cond),
            None => Err(ArmDecodeError::new(
                UndefinedConditionCode(cond_code as u32),
                raw,
                addr,
            )),
        }?;

        let fmt = if (0x0fff_fff0 & raw) == 0x012f_ff10 {
            Ok(BX)
        } else if (0x0e00_0000 & raw) == 0x0a00_0000 {
            Ok(B_BL)
        } else if (0xe000_0010 & raw) == 0x0600_0000 {
            Err(ArmDecodeError::new(UnknownInstructionFormat, raw, addr))
        } else if (0x0fb0_0ff0 & raw) == 0x0100_0090 {
            Ok(SWP)
        } else if (0x0fc0_00f0 & raw) == 0x0000_0090 {
            Ok(MUL_MLA)
        } else if (0x0f80_00f0 & raw) == 0x0080_0090 {
            Ok(MULL_MLAL)
        } else if (0x0fbf_0fff & raw) == 0x010f_0000 {
            Ok(MRS)
        } else if (0x0fbf_fff0 & raw) == 0x0129_f000 {
            Ok(MSR_REG)
        } else if (0x0dbf_f000 & raw) == 0x0128_f000 {
            Ok(MSR_FLAGS)
        } else if (0x0c00_0000 & raw) == 0x0400_0000 {
            Ok(LDR_STR)
        } else if (0x0e40_0F90 & raw) == 0x0000_0090 {
            Ok(LDR_STR_HS_REG)
        } else if (0x0e40_0090 & raw) == 0x0040_0090 {
            Ok(LDR_STR_HS_IMM)
        } else if (0x0e00_0000 & raw) == 0x0800_0000 {
            Ok(LDM_STM)
        } else if (0x0f00_0000 & raw) == 0x0f00_0000 {
            Ok(SWI)
        } else if (0x0c00_0000 & raw) == 0x0000_0000 {
            Ok(DP)
        } else {
            Err(ArmDecodeError::new(UnknownInstructionFormat, raw, addr))
        }?;

        Ok(ArmInstruction {
            cond: cond,
            fmt: fmt,
            raw: raw,
            pc: addr,
        })
    }

    fn decode_from_bytes(bytes: &[u8], addr: Addr) -> Result<Self, InstructionDecoderError> {
        let mut rdr = std::io::Cursor::new(bytes);
        let raw = rdr
            .read_u32::<LittleEndian>()
            .map_err(|e| InstructionDecoderError::IoError(e.kind()))?;
        Self::decode(raw, addr)
    }

    fn get_raw(&self) -> u32 {
        self.raw
    }
}

#[derive(Debug, PartialEq, Primitive)]
pub enum ArmShiftType {
    LSL = 0,
    LSR = 1,
    ASR = 2,
    ROR = 3,
}

#[derive(Debug, PartialEq)]
pub enum ArmRegisterShift {
    ShiftAmount(u32, ArmShiftType),
    ShiftRegister(usize, ArmShiftType),
}

impl TryFrom<u32> for ArmRegisterShift {
    type Error = ArmDecodeErrorKind;

    fn try_from(v: u32) -> Result<Self, Self::Error> {
        let typ = match ArmShiftType::from_u8(v.bit_range(5..7) as u8) {
            Some(s) => Ok(s),
            _ => Err(InvalidShiftType(v.bit_range(5..7))),
        }?;
        if v.bit(4) {
            let rs = v.bit_range(8..12) as usize;
            Ok(ArmRegisterShift::ShiftRegister(rs, typ))
        } else {
            let amount = v.bit_range(7..12) as u32;
            Ok(ArmRegisterShift::ShiftAmount(amount, typ))
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ArmShiftedValue {
    ImmediateValue(i32),
    RotatedImmediate(u32, u32),
    ShiftedRegister {
        reg: usize,
        shift: ArmRegisterShift,
        added: Option<bool>,
    },
}

impl ArmShiftedValue {
    /// Decode operand2 as an immediate value
    pub fn decode_rotated_immediate(&self) -> Option<i32> {
        if let ArmShiftedValue::RotatedImmediate(immediate, rotate) = self {
            return Some(immediate.rotate_right(*rotate) as i32);
        }
        None
    }
}

impl ArmInstruction {
    fn make_decode_error(&self, kind: ArmDecodeErrorKind) -> ArmDecodeError {
        ArmDecodeError {
            kind: kind,
            insn: self.raw,
            addr: self.pc,
        }
    }

    pub fn rn(&self) -> usize {
        match self.fmt {
            ArmFormat::MUL_MLA => self.raw.bit_range(12..16) as usize,
            ArmFormat::MULL_MLAL => self.raw.bit_range(8..12) as usize,
            ArmFormat::BX => self.raw.bit_range(0..4) as usize,
            _ => self.raw.bit_range(16..20) as usize,
        }
    }

    pub fn rd(&self) -> usize {
        match self.fmt {
            ArmFormat::MUL_MLA => self.raw.bit_range(16..20) as usize,
            _ => self.raw.bit_range(12..16) as usize,
        }
    }

    pub fn rm(&self) -> usize {
        self.raw.bit_range(0..4) as usize
    }

    pub fn rs(&self) -> usize {
        self.raw.bit_range(8..12) as usize
    }

    pub fn rd_lo(&self) -> usize {
        self.raw.bit_range(12..16) as usize
    }

    pub fn rd_hi(&self) -> usize {
        self.raw.bit_range(16..20) as usize
    }

    pub fn opcode(&self) -> Option<ArmOpCode> {
        ArmOpCode::from_u32(self.raw.bit_range(21..25))
    }

    pub fn branch_offset(&self) -> i32 {
        (((self.raw.bit_range(0..24) << 8) as i32) >> 6)
    }

    pub fn load_flag(&self) -> bool {
        self.raw.bit(20)
    }

    pub fn set_cond_flag(&self) -> bool {
        self.raw.bit(20)
    }

    pub fn write_back_flag(&self) -> bool {
        self.raw.bit(21)
    }

    pub fn accumulate_flag(&self) -> bool {
        self.raw.bit(21)
    }

    pub fn u_flag(&self) -> bool {
        self.raw.bit(22)
    }

    pub fn halfword_data_transfer_type(&self) -> Result<ArmHalfwordTransferType, ArmDecodeError> {
        let bits = (self.raw & 0b1100000) >> 5;
        match ArmHalfwordTransferType::from_u32(bits) {
            Some(x) => Ok(x),
            None => Err(ArmDecodeError::new(InvalidHSBits(bits), self.raw, self.pc)),
        }
    }

    pub fn transfer_size(&self) -> usize {
        if self.raw.bit(22) {
            1
        } else {
            4
        }
    }

    pub fn psr_and_force_user_flag(&self) -> bool {
        self.raw.bit(22)
    }

    pub fn spsr_flag(&self) -> bool {
        self.raw.bit(22)
    }

    pub fn add_offset_flag(&self) -> bool {
        self.raw.bit(23)
    }

    pub fn pre_index_flag(&self) -> bool {
        self.raw.bit(24)
    }

    pub fn link_flag(&self) -> bool {
        self.raw.bit(24)
    }

    /// gets offset used by ldr/str instructions
    pub fn ldr_str_offset(&self) -> Result<ArmShiftedValue, ArmDecodeError> {
        let ofs = self.raw.bit_range(0..12);
        if self.raw.bit(25) {
            let rm = ofs & 0xf;
            let shift =
                ArmRegisterShift::try_from(ofs).map_err(|kind| self.make_decode_error(kind))?;
            Ok(ArmShiftedValue::ShiftedRegister {
                reg: rm as usize,
                shift: shift,
                added: Some(self.add_offset_flag()),
            })
        } else {
            let ofs = if self.add_offset_flag() {
                ofs as i32
            } else {
                -(ofs as i32)
            };
            Ok(ArmShiftedValue::ImmediateValue(ofs))
        }
    }

    pub fn ldr_str_hs_offset(&self) -> Result<ArmShiftedValue, ArmDecodeError> {
        match self.fmt {
            ArmFormat::LDR_STR_HS_IMM => {
                let offset8 = (self.raw.bit_range(8..12) << 4) + self.raw.bit_range(0..4);
                let offset8 = if self.add_offset_flag() {
                    offset8 as i32
                } else {
                    -(offset8 as i32)
                };
                Ok(ArmShiftedValue::ImmediateValue(offset8))
            }
            ArmFormat::LDR_STR_HS_REG => Ok(ArmShiftedValue::ShiftedRegister {
                reg: (self.raw & 0xf) as usize,
                shift: ArmRegisterShift::ShiftAmount(0, ArmShiftType::LSL),
                added: Some(self.add_offset_flag()),
            }),
            _ => Err(self.make_decode_error(DecodedPartDoesNotBelongToInstruction)),
        }
    }

    pub fn operand2(&self) -> Result<ArmShiftedValue, ArmDecodeError> {
        let op2 = self.raw.bit_range(0..12);
        if self.raw.bit(25) {
            let immediate = op2 & 0xff;
            let rotate = 2 * op2.bit_range(8..12);
            Ok(ArmShiftedValue::RotatedImmediate(immediate, rotate))
        } else {
            let reg = op2 & 0xf;
            let shift =
                ArmRegisterShift::try_from(op2).map_err(|kind| self.make_decode_error(kind))?; // TODO error handling
            Ok(ArmShiftedValue::ShiftedRegister {
                reg: reg as usize,
                shift: shift,
                added: None,
            })
        }
    }

    pub fn register_list(&self) -> Vec<usize> {
        let list_bits = self.raw & 0xffff;
        let mut list = Vec::with_capacity(16);
        for i in 0..16 {
            if (list_bits & (1 << i)) != 0 {
                list.push(i)
            }
        }
        list
    }

    pub fn swi_comment(&self) -> u32 {
        self.raw.bit_range(0..24)
    }
}

#[cfg(test)]
/// All instructions constants were generated using an ARM assembler.
mod tests {
    use super::*;
    use crate::arm7tdmi::*;
    use crate::sysbus::BoxedMemory;

    #[test]
    fn test_decode_swi() {
        // swi #0x1337
        let decoded = ArmInstruction::decode(0xef001337, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::SWI);
        assert_eq!(decoded.swi_comment(), 0x1337);
        assert_eq!(format!("{}", decoded), "swi\t#0x1337");
    }

    #[test]
    fn test_decode_branch_forwards() {
        // 0x20:   b 0x30
        let decoded = ArmInstruction::decode(0xea_00_00_02, 0x20).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::B_BL);
        assert_eq!(decoded.link_flag(), false);
        assert_eq!(
            (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
            0x30
        );
        assert_eq!(format!("{}", decoded), "b\t0x30");
    }

    #[test]
    fn test_decode_branch_link_backwards() {
        // 0x20:   bl 0x10
        let decoded = ArmInstruction::decode(0xeb_ff_ff_fa, 0x20).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::B_BL);
        assert_eq!(decoded.link_flag(), true);
        assert_eq!(
            (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
            0x10
        );
        assert_eq!(format!("{}", decoded), "bl\t0x10");
    }

    #[test]
    fn test_decode_ldr_pre_index() {
        // ldreq r2, [r5, -r6, lsl #5]
        let decoded = ArmInstruction::decode(0x07_15_22_86, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::Equal);
        assert_eq!(decoded.load_flag(), true);
        assert_eq!(decoded.pre_index_flag(), true);
        assert_eq!(decoded.write_back_flag(), false);
        assert_eq!(decoded.rd(), 2);
        assert_eq!(decoded.rn(), 5);
        assert_eq!(
            decoded.ldr_str_offset(),
            Ok(ArmShiftedValue::ShiftedRegister {
                reg: 6,
                shift: ArmRegisterShift::ShiftAmount(5, ArmShiftType::LSL),
                added: Some(false)
            })
        );

        assert_eq!(format!("{}", decoded), "ldreq\tr2, [r5, -r6, lsl #5]");
    }

    #[test]
    fn test_decode_str_post_index() {
        // strteq r2, [r4], -r7, lsl #8
        let decoded = ArmInstruction::decode(0x06_24_24_47, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::Equal);
        assert_eq!(decoded.load_flag(), false);
        assert_eq!(decoded.pre_index_flag(), false);
        assert_eq!(decoded.write_back_flag(), true);
        assert_eq!(decoded.rd(), 2);
        assert_eq!(decoded.rn(), 4);
        assert_eq!(
            decoded.ldr_str_offset(),
            Ok(ArmShiftedValue::ShiftedRegister {
                reg: 7,
                shift: ArmRegisterShift::ShiftAmount(8, ArmShiftType::ASR),
                added: Some(false)
            })
        );

        assert_eq!(format!("{}", decoded), "strteq\tr2, [r4], -r7, asr #8");
    }

    #[test]
    fn str_pre_index() {
        // str r4, [sp, 0x10]
        let decoded = ArmInstruction::decode(0xe58d4010, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::Always);

        let mut core = Core::new();
        core.set_reg(4, 0x12345678);
        core.set_reg(REG_SP, 0);

        let bytes = vec![
            /*  0: */ 0xaa, 0xbb, 0xcc, 0xdd, /*  4: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /*  8: */ 0xaa, 0xbb, 0xcc, 0xdd, /*  c: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /* 10: */ 0xaa, 0xbb, 0xcc, 0xdd,
        ];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice());

        assert_ne!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
        assert_eq!(
            core.exec_arm(&mut mem, decoded),
            Ok(CpuPipelineAction::IncPC)
        );
        assert_eq!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
    }
}
