use std::fmt;

use super::super::{reg_string, REG_PC};
use super::{
    ArmCond, ArmHalfwordTransferType, ArmInstruction, ArmInstructionFormat, ArmOpCode,
    ArmRegisterShift, ArmShiftType, ArmShiftedValue,
};

impl fmt::Display for ArmCond {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmCond::*;
        match self {
            Equal => write!(f, "eq"),
            NotEqual => write!(f, "ne"),
            UnsignedHigherOrSame => write!(f, "cs"),
            UnsignedLower => write!(f, "cc"),
            Negative => write!(f, "mi"),
            PositiveOrZero => write!(f, "pl"),
            Overflow => write!(f, "vs"),
            NoOverflow => write!(f, "vc"),
            UnsignedHigher => write!(f, "hi"),
            UnsignedLowerOrSame => write!(f, "ls"),
            GreaterOrEqual => write!(f, "ge"),
            LessThan => write!(f, "lt"),
            GreaterThan => write!(f, "gt"),
            LessThanOrEqual => write!(f, "le"),
            Always => write!(f, ""), // the dissasembly should ignore this
        }
    }
}

impl fmt::Display for ArmOpCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmOpCode::*;
        match self {
            AND => write!(f, "and"),
            EOR => write!(f, "eor"),
            SUB => write!(f, "sub"),
            RSB => write!(f, "rsb"),
            ADD => write!(f, "add"),
            ADC => write!(f, "adc"),
            SBC => write!(f, "sbc"),
            RSC => write!(f, "rsc"),
            TST => write!(f, "tst"),
            TEQ => write!(f, "teq"),
            CMP => write!(f, "cmp"),
            CMN => write!(f, "cmn"),
            ORR => write!(f, "orr"),
            MOV => write!(f, "mov"),
            BIC => write!(f, "bic"),
            MVN => write!(f, "mvn"),
        }
    }
}

impl fmt::Display for ArmShiftType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmShiftType::*;
        match self {
            LSL => write!(f, "lsl"),
            LSR => write!(f, "lsr"),
            ASR => write!(f, "asr"),
            ROR => write!(f, "ror"),
        }
    }
}

impl fmt::Display for ArmHalfwordTransferType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmHalfwordTransferType::*;
        match self {
            UnsignedHalfwords => write!(f, "h"),
            SignedHalfwords => write!(f, "sh"),
            SignedByte => write!(f, "sb"),
        }
    }
}

fn is_shift(shift: &ArmRegisterShift) -> bool {
    if let ArmRegisterShift::ShiftAmount(val, typ) = shift {
        return !(*val == 0 && *typ == ArmShiftType::LSL);
    }
    true
}

impl ArmInstruction {
    fn make_shifted_reg_string(&self, reg: usize, shift: ArmRegisterShift) -> String {
        let reg = reg_string(reg).to_string();
        if !is_shift(&shift) {
            return reg;
        }

        match shift {
            ArmRegisterShift::ShiftAmount(imm, typ) => format!("{}, {} #{}", reg, typ, imm),
            ArmRegisterShift::ShiftRegister(rs, typ) => {
                format!("{}, {} {}", reg, typ, reg_string(rs))
            }
        }
    }

    fn fmt_bx(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "bx\t{Rn}", Rn = reg_string(self.rn()))
    }

    fn fmt_branch(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "b{link}{cond}\t{ofs:#x}",
            link = if self.link_flag() { "l" } else { "" },
            cond = self.cond,
            ofs = 8 + self.pc.wrapping_add(self.branch_offset() as u32) as u32
        )
    }

    fn set_cond_mark(&self) -> &str {
        if self.set_cond_flag() {
            "s"
        } else {
            ""
        }
    }

    fn fmt_data_processing(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmOpCode::*;

        let opcode = self.opcode().unwrap();

        match opcode {
            MOV | MVN => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, ",
                opcode = opcode,
                cond = self.cond,
                S = self.set_cond_mark(),
                Rd = reg_string(self.rd())
            ),
            CMP | CMN | TEQ | TST => write!(
                f,
                "{opcode}{cond}\t{Rn}, ",
                opcode = opcode,
                cond = self.cond,
                Rn = reg_string(self.rn())
            ),
            _ => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, {Rn}, ",
                opcode = opcode,
                cond = self.cond,
                S = self.set_cond_mark(),
                Rd = reg_string(self.rd()),
                Rn = reg_string(self.rn())
            ),
        }?;

        let operand2 = self.operand2().unwrap();
        match operand2 {
            ArmShiftedValue::RotatedImmediate(_, _) => {
                let value = operand2.decode_rotated_immediate().unwrap();
                write!(f, "#{}\t; {:#x}", value, value)
            }
            ArmShiftedValue::ShiftedRegister {
                reg,
                shift,
                added: _,
            } => write!(f, "{}", self.make_shifted_reg_string(reg, shift)),
            _ => write!(f, "RegisterNotImpl"),
        }
    }

    fn auto_incremenet_mark(&self) -> &str {
        if self.write_back_flag() {
            "!"
        } else {
            ""
        }
    }

    fn fmt_rn_offset(&self, f: &mut fmt::Formatter, offset: ArmShiftedValue) -> fmt::Result {
        write!(f, "[{Rn}", Rn = reg_string(self.rn()))?;
        let (ofs_string, comment) = match offset {
            ArmShiftedValue::ImmediateValue(value) => {
                let value_for_commnet = if self.rn() == REG_PC {
                    value + (self.pc as i32) + 8 // account for pipelining
                } else {
                    value
                };
                (
                    format!("#{}", value),
                    Some(format!("\t; {:#x}", value_for_commnet)),
                )
            }
            ArmShiftedValue::ShiftedRegister {
                reg,
                shift,
                added: Some(added),
            } => (
                format!(
                    "{}{}",
                    if added { "" } else { "-" },
                    self.make_shifted_reg_string(reg, shift)
                ),
                None,
            ),
            _ => panic!("bad barrel shifter"),
        };

        if self.pre_index_flag() {
            write!(f, ", {}]{}", ofs_string, self.auto_incremenet_mark())?;
        } else {
            write!(f, "], {}", ofs_string)?;
        }

        if let Some(comment) = comment {
            write!(f, "{}", comment)
        } else {
            Ok(())
        }
    }

    fn fmt_ldr_str(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{mnem}{B}{T}{cond}\t{Rd}, ",
            mnem = if self.load_flag() { "ldr" } else { "str" },
            B = if self.transfer_size() == 1 { "b" } else { "" },
            cond = self.cond,
            T = if !self.pre_index_flag() && self.write_back_flag() {
                "t"
            } else {
                ""
            },
            Rd = reg_string(self.rd()),
        )?;

        self.fmt_rn_offset(f, self.ldr_str_offset().unwrap())
    }

    fn fmt_ldm_stm(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{mnem}{inc_dec}{pre_post}{cond}\t{Rn}{auto_inc}, {{",
            mnem = if self.load_flag() { "ldm" } else { "stm" },
            inc_dec = if self.add_offset_flag() { 'i' } else { 'd' },
            pre_post = if self.pre_index_flag() { 'b' } else { 'a' },
            cond = self.cond,
            Rn = reg_string(self.rn()),
            auto_inc = if self.write_back_flag() { "!" } else { "" }
        )?;

        let mut register_list = self.register_list().into_iter();
        if let Some(reg) = register_list.next() {
            write!(f, "{}", reg_string(reg))?;
        }
        for reg in register_list {
            write!(f, ", {}", reg_string(reg))?;
        }
        write!(f, "}}")
    }

    /// MRS - transfer PSR contents to a register
    fn fmt_mrs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "mrs{cond}\t{Rd}, {psr}",
            cond = self.cond,
            Rd = reg_string(self.rd()),
            psr = if self.spsr_flag() { "SPSR" } else { "CPSR" }
        )
    }

    /// MSR - transfer register contents to PSR
    fn fmt_msr_reg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, {Rm}",
            cond = self.cond,
            psr = if self.spsr_flag() { "SPSR" } else { "CPSR" },
            Rm = reg_string(self.rm()),
        )
    }

    fn fmt_mul_mla(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.accumulate_flag() {
            write!(
                f,
                "mla{S}{cond}\t{Rd}, {Rm}, {Rs}, {Rn}",
                S = self.set_cond_mark(),
                cond = self.cond,
                Rd = reg_string(self.rd()),
                Rm = reg_string(self.rm()),
                Rs = reg_string(self.rs()),
                Rn = reg_string(self.rn()),
            )
        } else {
            write!(
                f,
                "mul{S}{cond}\t{Rd}, {Rm}, {Rs}",
                S = self.set_cond_mark(),
                cond = self.cond,
                Rd = reg_string(self.rd()),
                Rm = reg_string(self.rm()),
                Rs = reg_string(self.rs()),
            )
        }
    }

    fn sign_mark(&self) -> &str {
        if self.u_flag() {
            "s"
        } else {
            "u"
        }
    }

    fn fmt_mull_mlal(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.accumulate_flag() {
            write!(
                f,
                "{sign}mlal{S}{cond}\t{RdLo}, {RdHi}, {Rm}, {Rs}",
                sign = self.sign_mark(),
                S = self.set_cond_mark(),
                cond = self.cond,
                RdLo = reg_string(self.rd_lo()),
                RdHi = reg_string(self.rd_hi()),
                Rm = reg_string(self.rm()),
                Rs = reg_string(self.rs()),
            )
        } else {
            write!(
                f,
                "{sign}mull{S}{cond}\t{RdLo}, {RdHi}, {Rm}",
                sign = self.sign_mark(),
                S = self.set_cond_mark(),
                cond = self.cond,
                RdLo = reg_string(self.rd_lo()),
                RdHi = reg_string(self.rd_hi()),
                Rm = reg_string(self.rm())
            )
        }
    }

    fn fmt_ldr_str_hs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Ok(transfer_type) = self.halfword_data_transfer_type() {
            write!(
                f,
                "{mnem}{type}{cond}\t{Rd}, ",
                mnem = if self.load_flag() { "ldr" } else { "str" },
                cond = self.cond,
                type = transfer_type,
                Rd = reg_string(self.rd()),
            )?;
            self.fmt_rn_offset(f, self.ldr_str_hs_offset().unwrap())
        } else {
            write!(f, "<undefined>")
        }
    }

    fn fmt_swi(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "swi{cond}\t#{comm:#x}",
            cond = self.cond,
            comm = self.swi_comment()
        )
    }
}

impl fmt::Display for ArmInstruction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmInstructionFormat::*;
        match self.fmt {
            BX => self.fmt_bx(f),
            B_BL => self.fmt_branch(f),
            DP => self.fmt_data_processing(f),
            LDR_STR => self.fmt_ldr_str(f),
            LDM_STM => self.fmt_ldm_stm(f),
            MRS => self.fmt_mrs(f),
            MSR_REG => self.fmt_msr_reg(f),
            MUL_MLA => self.fmt_mul_mla(f),
            MULL_MLAL => self.fmt_mull_mlal(f),
            LDR_STR_HS_IMM => self.fmt_ldr_str_hs(f),
            LDR_STR_HS_REG => self.fmt_ldr_str_hs(f),
            SWI => self.fmt_swi(f),
            _ => write!(f, "({:?})", self),
        }
    }
}
