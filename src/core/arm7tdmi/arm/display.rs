use std::fmt;

#[cfg(feature = "debugger")]
use crate::bit::BitIndex;

#[cfg(feature = "debugger")]
use super::{ArmFormat, ArmInstruction};

use super::{AluOpCode, ArmCond, ArmHalfwordTransferType};
use crate::core::arm7tdmi::*;

impl fmt::Display for ArmCond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArmCond::*;
        match self {
            EQ => write!(f, "eq"),
            NE => write!(f, "ne"),
            HS => write!(f, "cs"),
            LO => write!(f, "cc"),
            MI => write!(f, "mi"),
            PL => write!(f, "pl"),
            VS => write!(f, "vs"),
            VC => write!(f, "vc"),
            HI => write!(f, "hi"),
            LS => write!(f, "ls"),
            GE => write!(f, "ge"),
            LT => write!(f, "lt"),
            GT => write!(f, "gt"),
            LE => write!(f, "le"),
            AL => write!(f, ""), // the dissasembly should ignore this
        }
    }
}

impl fmt::Display for AluOpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use AluOpCode::*;
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

impl fmt::Display for BarrelShiftOpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use BarrelShiftOpCode::*;
        match self {
            LSL => write!(f, "lsl"),
            LSR => write!(f, "lsr"),
            ASR => write!(f, "asr"),
            ROR => write!(f, "ror"),
        }
    }
}

impl fmt::Display for ArmHalfwordTransferType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArmHalfwordTransferType::*;
        match self {
            UnsignedHalfwords => write!(f, "h"),
            SignedHalfwords => write!(f, "sh"),
            SignedByte => write!(f, "sb"),
        }
    }
}

fn is_lsl0(shift: &ShiftedRegister) -> bool {
    if let ShiftRegisterBy::ByAmount(val) = shift.shift_by {
        return !(val == 0 && shift.bs_op == BarrelShiftOpCode::LSL);
    }
    true
}

impl fmt::Display for ShiftedRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reg = reg_string(self.reg).to_string();
        if !is_lsl0(&self) {
            write!(f, "{}", reg)
        } else {
            match self.shift_by {
                ShiftRegisterBy::ByAmount(imm) => write!(f, "{}, {} #{}", reg, self.bs_op, imm),
                ShiftRegisterBy::ByRegister(rs) => {
                    write!(f, "{}, {} {}", reg, self.bs_op, reg_string(rs))
                }
            }
        }
    }
}

#[cfg(feature = "debugger")]
impl ArmInstruction {
    fn fmt_bx(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bx\t{Rn}", Rn = reg_string(self.rn()))
    }

    fn fmt_branch(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "b{link}{cond}\t{ofs:#x}",
            link = if self.link_flag() { "l" } else { "" },
            cond = self.cond(),
            ofs = 8 + self.pc.wrapping_add(self.branch_offset() as Addr)
        )
    }

    fn set_cond_mark(&self) -> &str {
        if self.set_cond_flag() {
            "s"
        } else {
            ""
        }
    }

    fn fmt_operand2(&self, f: &mut fmt::Formatter<'_>) -> Result<Option<u32>, fmt::Error> {
        let operand2 = self.operand2();
        match operand2 {
            BarrelShifterValue::RotatedImmediate(_, _) => {
                let value = operand2.decode_rotated_immediate().unwrap();
                write!(f, "#{}\t; {:#x}", value, value)?;
                Ok(Some(value as u32))
            }
            BarrelShifterValue::ShiftedRegister(shift) => {
                write!(f, "{}", shift)?;
                Ok(None)
            }
            _ => panic!("invalid operand2"),
        }
    }

    fn fmt_data_processing(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use AluOpCode::*;

        let opcode = self.opcode().unwrap();

        match opcode {
            MOV | MVN => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, ",
                opcode = opcode,
                cond = self.cond(),
                S = self.set_cond_mark(),
                Rd = reg_string(self.rd())
            ),
            CMP | CMN | TEQ | TST => write!(
                f,
                "{opcode}{cond}\t{Rn}, ",
                opcode = opcode,
                cond = self.cond(),
                Rn = reg_string(self.rn())
            ),
            _ => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, {Rn}, ",
                opcode = opcode,
                cond = self.cond(),
                S = self.set_cond_mark(),
                Rd = reg_string(self.rd()),
                Rn = reg_string(self.rn())
            ),
        }?;

        self.fmt_operand2(f).unwrap();
        Ok(())
    }

    fn auto_incremenet_mark(&self) -> &str {
        if self.write_back_flag() {
            "!"
        } else {
            ""
        }
    }

    fn fmt_rn_offset(&self, f: &mut fmt::Formatter<'_>, offset: BarrelShifterValue) -> fmt::Result {
        write!(f, "[{Rn}", Rn = reg_string(self.rn()))?;
        let (ofs_string, comment) = match offset {
            BarrelShifterValue::ImmediateValue(value) => {
                let value_for_commnet = if self.rn() == REG_PC {
                    value + self.pc + 8 // account for pipelining
                } else {
                    value
                };
                (
                    format!("#{}", value),
                    Some(format!("\t; {:#x}", value_for_commnet)),
                )
            }
            BarrelShifterValue::ShiftedRegister(shift) => (
                format!(
                    "{}{}",
                    if shift.added.unwrap_or(true) { "" } else { "-" },
                    shift
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

    fn fmt_ldr_str(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{mnem}{B}{T}{cond}\t{Rd}, ",
            mnem = if self.load_flag() { "ldr" } else { "str" },
            B = if self.transfer_size() == 1 { "b" } else { "" },
            cond = self.cond(),
            T = if !self.pre_index_flag() && self.write_back_flag() {
                "t"
            } else {
                ""
            },
            Rd = reg_string(self.rd()),
        )?;

        self.fmt_rn_offset(f, self.ldr_str_offset())
    }

    fn fmt_ldm_stm(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{mnem}{inc_dec}{pre_post}{cond}\t{Rn}{auto_inc}, {{",
            mnem = if self.load_flag() { "ldm" } else { "stm" },
            inc_dec = if self.add_offset_flag() { 'i' } else { 'd' },
            pre_post = if self.pre_index_flag() { 'b' } else { 'a' },
            cond = self.cond(),
            Rn = reg_string(self.rn()),
            auto_inc = if self.write_back_flag() { "!" } else { "" }
        )?;

        let register_list = self.register_list();
        let mut has_first = false;
        for i in 0..16 {
            if register_list.bit(i) {
                if has_first {
                    write!(f, ", {}", reg_string(i))?;
                } else {
                    write!(f, "{}", reg_string(i))?;
                    has_first = true;
                }
            }
        }

        write!(
            f,
            "}}{}",
            if self.psr_and_force_user_flag() {
                "^"
            } else {
                ""
            }
        )
    }

    /// MRS - transfer PSR contents to a register
    fn fmt_mrs(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mrs{cond}\t{Rd}, {psr}",
            cond = self.cond(),
            Rd = reg_string(self.rd()),
            psr = if self.spsr_flag() { "SPSR" } else { "CPSR" }
        )
    }

    /// MSR - transfer register contents to PSR
    fn fmt_msr_reg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, {Rm}",
            cond = self.cond(),
            psr = if self.spsr_flag() { "SPSR" } else { "CPSR" },
            Rm = reg_string(self.rm()),
        )
    }

    fn fmt_msr_flags(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, ",
            cond = self.cond(),
            psr = if self.spsr_flag() { "SPSR_f" } else { "CPSR_f" },
        )?;
        if let Ok(Some(op)) = self.fmt_operand2(f) {
            let psr = RegPSR::new(op & 0xf000_0000);
            write!(
                f,
                "\t; N={} Z={} C={} V={}",
                psr.N(),
                psr.Z(),
                psr.C(),
                psr.V()
            )?;
        }
        Ok(())
    }

    fn fmt_mul_mla(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.accumulate_flag() {
            write!(
                f,
                "mla{S}{cond}\t{Rd}, {Rm}, {Rs}, {Rn}",
                S = self.set_cond_mark(),
                cond = self.cond(),
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
                cond = self.cond(),
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

    fn fmt_mull_mlal(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{sign}{mnem}{S}{cond}\t{RdLo}, {RdHi}, {Rm}, {Rs}",
            sign = self.sign_mark(),
            mnem = if self.accumulate_flag() {
                "mlal"
            } else {
                "mull"
            },
            S = self.set_cond_mark(),
            cond = self.cond(),
            RdLo = reg_string(self.rd_lo()),
            RdHi = reg_string(self.rd_hi()),
            Rm = reg_string(self.rm()),
            Rs = reg_string(self.rs()),
        )
    }

    fn fmt_ldr_str_hs(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Ok(transfer_type) = self.halfword_data_transfer_type() {
            write!(
                f,
                "{mnem}{type}{cond}\t{Rd}, ",
                mnem = if self.load_flag() { "ldr" } else { "str" },
                cond = self.cond(),
                type = transfer_type,
                Rd = reg_string(self.rd()),
            )?;
            self.fmt_rn_offset(f, self.ldr_str_hs_offset().unwrap())
        } else {
            write!(f, "<undefined>")
        }
    }

    fn fmt_swi(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "swi{cond}\t#{comm:#x}",
            cond = self.cond(),
            comm = self.swi_comment()
        )
    }

    fn fmt_swp(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "swp{B}{cond}\t{Rd}, {Rm}, [{Rn}]",
            B = if self.transfer_size() == 1 { "b" } else { "" },
            cond = self.cond(),
            Rd = reg_string(self.rd()),
            Rm = reg_string(self.rm()),
            Rn = reg_string(self.rn()),
        )
    }
}

#[cfg(feature = "debugger")]
impl fmt::Display for ArmInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArmFormat::*;
        match self.fmt {
            BX => self.fmt_bx(f),
            B_BL => self.fmt_branch(f),
            DP => self.fmt_data_processing(f),
            LDR_STR => self.fmt_ldr_str(f),
            LDM_STM => self.fmt_ldm_stm(f),
            MRS => self.fmt_mrs(f),
            MSR_REG => self.fmt_msr_reg(f),
            MSR_FLAGS => self.fmt_msr_flags(f),
            MUL_MLA => self.fmt_mul_mla(f),
            MULL_MLAL => self.fmt_mull_mlal(f),
            LDR_STR_HS_IMM => self.fmt_ldr_str_hs(f),
            LDR_STR_HS_REG => self.fmt_ldr_str_hs(f),
            SWI => self.fmt_swi(f),
            SWP => self.fmt_swp(f),
            Undefined => write!(f, "<Undefined>"),
        }
    }
}
