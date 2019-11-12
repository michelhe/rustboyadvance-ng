pub mod display;
pub mod exec;

use super::alu::*;
use crate::core::arm7tdmi::{Addr, InstructionDecoder, InstructionDecoderError};

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

    pub fn opcode(&self) -> Option<AluOpCode> {
        AluOpCode::from_u32(self.raw.bit_range(21..25))
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
    pub fn ldr_str_offset(&self) -> BarrelShifterValue {
        let ofs = self.raw.bit_range(0..12);
        if self.raw.bit(25) {
            let rm = ofs & 0xf;
            BarrelShifterValue::ShiftedRegister(ShiftedRegister {
                reg: rm as usize,
                shift_by: self.get_shift_reg_by(ofs),
                bs_op: self.get_bs_op(ofs),
                added: Some(self.add_offset_flag()),
            })
        } else {
            let ofs = if self.add_offset_flag() {
                ofs as u32
            } else {
                (-(ofs as i32) as u32)
            };
            BarrelShifterValue::ImmediateValue(ofs)
        }
    }

    fn get_bs_op(&self, shift_field: u32) -> BarrelShiftOpCode {
        BarrelShiftOpCode::from_u8(shift_field.bit_range(5..7) as u8).unwrap()
    }

    fn get_shift_reg_by(&self, shift_field: u32) -> ShiftRegisterBy {
        if shift_field.bit(4) {
            let rs = shift_field.bit_range(8..12) as usize;
            ShiftRegisterBy::ByRegister(rs)
        } else {
            let amount = shift_field.bit_range(7..12) as u32;
            ShiftRegisterBy::ByAmount(amount)
        }
    }

    pub fn ldr_str_hs_offset(&self) -> Result<BarrelShifterValue, ArmDecodeError> {
        match self.fmt {
            ArmFormat::LDR_STR_HS_IMM => {
                let offset8 = (self.raw.bit_range(8..12) << 4) + self.raw.bit_range(0..4);
                let offset8 = if self.add_offset_flag() {
                    offset8
                } else {
                    (-(offset8 as i32)) as u32
                };
                Ok(BarrelShifterValue::ImmediateValue(offset8))
            }
            ArmFormat::LDR_STR_HS_REG => Ok(BarrelShifterValue::ShiftedRegister(ShiftedRegister {
                reg: (self.raw & 0xf) as usize,
                shift_by: ShiftRegisterBy::ByAmount(0),
                bs_op: BarrelShiftOpCode::LSL,
                added: Some(self.add_offset_flag()),
            })),
            _ => Err(self.make_decode_error(DecodedPartDoesNotBelongToInstruction)),
        }
    }

    pub fn operand2(&self) -> Result<BarrelShifterValue, ArmDecodeError> {
        let op2 = self.raw.bit_range(0..12);
        if self.raw.bit(25) {
            let immediate = op2 & 0xff;
            let rotate = 2 * op2.bit_range(8..12);
            Ok(BarrelShifterValue::RotatedImmediate(immediate, rotate))
        } else {
            let reg = op2 & 0xf;
            let shifted_reg = ShiftedRegister {
                reg: reg as usize,
                bs_op: self.get_bs_op(op2),
                shift_by: self.get_shift_reg_by(op2),
                added: None,
            }; // TODO error handling
            Ok(BarrelShifterValue::ShiftedRegister(shifted_reg))
        }
    }

    pub fn register_list(&self) -> u16 {
        (self.raw & 0xffff) as u16
    }

    pub fn swi_comment(&self) -> u32 {
        self.raw.bit_range(0..24)
    }
}

#[cfg(test)]
/// All instructions constants were generated using an ARM assembler.
mod tests {
    use super::*;
    use crate::core::arm7tdmi::*;
    use crate::core::sysbus::BoxedMemory;

    #[test]
    fn swi() {
        let mut core = Core::new();

        let bytes = vec![];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        // swi #0x1337
        let decoded = ArmInstruction::decode(0xef001337, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::SWI);
        assert_eq!(decoded.swi_comment(), 0x1337);
        assert_eq!(format!("{}", decoded), "swi\t#0x1337");

        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(core.did_pipeline_flush(), true);

        assert_eq!(core.cpsr.mode(), CpuMode::Supervisor);
        assert_eq!(core.pc, Exception::SoftwareInterrupt as u32);
    }

    #[test]
    fn branch_forwards() {
        // 0x20:   b 0x30
        let decoded = ArmInstruction::decode(0xea_00_00_02, 0x20).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::B_BL);
        assert_eq!(decoded.link_flag(), false);
        assert_eq!(
            (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
            0x30
        );
        assert_eq!(format!("{}", decoded), "b\t0x30");

        let mut core = Core::new();
        core.pc = 0x20 + 8;

        let bytes = vec![];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(core.did_pipeline_flush(), true);
        assert_eq!(core.pc, 0x30);
    }

    #[test]
    fn branch_link_backwards() {
        // 0x20:   bl 0x10
        let decoded = ArmInstruction::decode(0xeb_ff_ff_fa, 0x20).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::B_BL);
        assert_eq!(decoded.link_flag(), true);
        assert_eq!(
            (decoded.pc as i32).wrapping_add(decoded.branch_offset()) + 8,
            0x10
        );
        assert_eq!(format!("{}", decoded), "bl\t0x10");

        let mut core = Core::new();
        core.pc = 0x20 + 8;

        let bytes = vec![];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(core.did_pipeline_flush(), true);
        assert_eq!(core.pc, 0x10);
    }

    #[test]
    fn ldr_pre_index() {
        // ldreq r2, [r5, -r6, lsl #5]
        let decoded = ArmInstruction::decode(0x07_15_22_86, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::EQ);
        assert_eq!(decoded.load_flag(), true);
        assert_eq!(decoded.pre_index_flag(), true);
        assert_eq!(decoded.write_back_flag(), false);
        assert_eq!(decoded.rd(), 2);
        assert_eq!(decoded.rn(), 5);
        assert_eq!(
            decoded.ldr_str_offset(),
            BarrelShifterValue::ShiftedRegister(ShiftedRegister {
                reg: 6,
                shift_by: ShiftRegisterBy::ByAmount(5),
                bs_op: BarrelShiftOpCode::LSL,
                added: Some(false)
            })
        );

        assert_eq!(format!("{}", decoded), "ldreq\tr2, [r5, -r6, lsl #5]");

        let mut core = Core::new();
        core.cpsr.set_Z(true);
        core.gpr[5] = 0x34;
        core.gpr[6] = 1;
        core.gpr[2] = 0;

        #[rustfmt::skip]
        let bytes = vec![
            /* 00h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 10h: */ 0x00, 0x00, 0x00, 0x00, 0x37, 0x13, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 20h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 30h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(core.gpr[2], 0x1337);
    }

    #[test]
    fn str_post_index() {
        // strteq r2, [r4], -r7, asr #8
        let decoded = ArmInstruction::decode(0x06_24_24_47, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::EQ);
        assert_eq!(decoded.load_flag(), false);
        assert_eq!(decoded.pre_index_flag(), false);
        assert_eq!(decoded.write_back_flag(), true);
        assert_eq!(decoded.rd(), 2);
        assert_eq!(decoded.rn(), 4);
        assert_eq!(
            decoded.ldr_str_offset(),
            BarrelShifterValue::ShiftedRegister(ShiftedRegister {
                reg: 7,
                shift_by: ShiftRegisterBy::ByAmount(8),
                bs_op: BarrelShiftOpCode::ASR,
                added: Some(false)
            })
        );
        assert_eq!(format!("{}", decoded), "strteq\tr2, [r4], -r7, asr #8");

        let mut core = Core::new();
        core.cpsr.set_Z(true);
        core.gpr[4] = 0x0;
        core.gpr[7] = 1;
        core.gpr[2] = 0xabababab;

        #[rustfmt::skip]
        let bytes = vec![
            /* 00h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 10h: */ 0x00, 0x00, 0x00, 0x00, 0x37, 0x13, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 20h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            /* 30h: */ 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(mem.read_32(0), 0xabababab);
    }

    #[test]
    fn str_pre_index() {
        // str r4, [sp, 0x10]
        let decoded = ArmInstruction::decode(0xe58d4010, 0).unwrap();
        assert_eq!(decoded.fmt, ArmFormat::LDR_STR);
        assert_eq!(decoded.cond, ArmCond::AL);

        let mut core = Core::new();
        core.set_reg(4, 0x12345678);
        core.set_reg(REG_SP, 0);

        #[rustfmt::skip]
        let bytes = vec![
            /*  0: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /*  4: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /*  8: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /*  c: */ 0xaa, 0xbb, 0xcc, 0xdd,
            /* 10: */ 0xaa, 0xbb, 0xcc, 0xdd,
        ];
        let mut mem = BoxedMemory::new(bytes.into_boxed_slice(), 0xffff_ffff);

        assert_ne!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
        core.exec_arm(&mut mem, decoded).unwrap();
        assert_eq!(mem.read_32(core.get_reg(REG_SP) + 0x10), 0x12345678);
    }
}
