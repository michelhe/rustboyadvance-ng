use std::fmt;

use crate::bit::BitIndex;

use super::{ArmDecodeHelper, ArmFormat, ArmInstruction};

use super::{AluOpCode, ArmCond, ArmHalfwordTransferType};
use crate::arm7tdmi::*;

impl fmt::Display for ArmCond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArmCond::*;
        match self {
            Invalid => panic!("Invalid condition code"),
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

impl ArmInstruction {
    fn fmt_bx(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bx\t{Rn}",
            Rn = reg_string(self.raw.bit_range(0..4) as usize)
        )
    }

    fn fmt_branch(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "b{link}{cond}\t{ofs:#x}",
            link = if self.raw.link_flag() { "l" } else { "" },
            cond = self.raw.cond(),
            ofs = 8 + self.pc.wrapping_add(self.raw.branch_offset() as Addr)
        )
    }

    fn set_cond_mark(&self) -> &str {
        if self.raw.set_cond_flag() {
            "s"
        } else {
            ""
        }
    }

    fn fmt_operand2(&self, f: &mut fmt::Formatter<'_>) -> Result<Option<u32>, fmt::Error> {
        let operand2 = self.raw.operand2();
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

        let opcode = self.raw.opcode();

        let rd = self.raw.bit_range(16..20) as usize;
        let rn = self.raw.bit_range(16..20) as usize;

        match opcode {
            MOV | MVN => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, ",
                opcode = opcode,
                cond = self.raw.cond(),
                S = self.set_cond_mark(),
                Rd = reg_string(rd)
            ),
            CMP | CMN | TEQ | TST => write!(
                f,
                "{opcode}{cond}\t{Rn}, ",
                opcode = opcode,
                cond = self.raw.cond(),
                Rn = reg_string(rn)
            ),
            _ => write!(
                f,
                "{opcode}{S}{cond}\t{Rd}, {Rn}, ",
                opcode = opcode,
                cond = self.raw.cond(),
                S = self.set_cond_mark(),
                Rd = reg_string(rd),
                Rn = reg_string(rn)
            ),
        }?;

        self.fmt_operand2(f).unwrap();
        Ok(())
    }

    fn auto_incremenet_mark(&self) -> &str {
        if self.raw.write_back_flag() {
            "!"
        } else {
            ""
        }
    }

    fn fmt_rn_offset(
        &self,
        f: &mut fmt::Formatter<'_>,
        offset: BarrelShifterValue,
        rn: usize,
    ) -> fmt::Result {
        write!(f, "[{Rn}", Rn = reg_string(rn))?;
        let (ofs_string, comment) = match offset {
            BarrelShifterValue::ImmediateValue(value) => {
                let value_for_commnet = if rn == REG_PC {
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

        if self.raw.pre_index_flag() {
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
            mnem = if self.raw.load_flag() { "ldr" } else { "str" },
            B = if self.raw.transfer_size() == 1 {
                "b"
            } else {
                ""
            },
            cond = self.raw.cond(),
            T = if !self.raw.pre_index_flag() && self.raw.write_back_flag() {
                "t"
            } else {
                ""
            },
            Rd = reg_string(self.raw.bit_range(12..16) as usize),
        )?;

        self.fmt_rn_offset(
            f,
            self.raw.ldr_str_offset(),
            self.raw.bit_range(16..20) as usize,
        )
    }

    fn fmt_ldm_stm(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{mnem}{inc_dec}{pre_post}{cond}\t{Rn}{auto_inc}, {{",
            mnem = if self.raw.load_flag() { "ldm" } else { "stm" },
            inc_dec = if self.raw.add_offset_flag() { 'i' } else { 'd' },
            pre_post = if self.raw.pre_index_flag() { 'b' } else { 'a' },
            cond = self.raw.cond(),
            Rn = reg_string(self.raw.bit_range(16..20) as usize),
            auto_inc = if self.raw.write_back_flag() { "!" } else { "" }
        )?;

        let register_list = self.raw.register_list();
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
            if self.raw.psr_and_force_user_flag() {
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
            cond = self.raw.cond(),
            Rd = reg_string(self.raw.bit_range(12..16) as usize),
            psr = if self.raw.spsr_flag() { "SPSR" } else { "CPSR" }
        )
    }

    /// MSR - transfer register/immediate contents to PSR
    fn fmt_msr(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, ",
            cond = self.raw.cond(),
            psr = if self.raw.spsr_flag() { "SPSR" } else { "CPSR" },
        )?;
        self.fmt_operand2(f).unwrap();
        Ok(())
    }

    fn fmt_msr_flags(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, ",
            cond = self.raw.cond(),
            psr = if self.raw.spsr_flag() {
                "SPSR_f"
            } else {
                "CPSR_f"
            },
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
        let rd = self.raw.bit_range(16..20) as usize;
        if self.raw.accumulate_flag() {
            write!(
                f,
                "mla{S}{cond}\t{Rd}, {Rm}, {Rs}, {Rn}",
                S = self.set_cond_mark(),
                cond = self.raw.cond(),
                Rd = reg_string(rd),
                Rm = reg_string(self.raw.rm()),
                Rs = reg_string(self.raw.rs()),
                Rn = reg_string(self.raw.bit_range(12..16) as usize),
            )
        } else {
            write!(
                f,
                "mul{S}{cond}\t{Rd}, {Rm}, {Rs}",
                S = self.set_cond_mark(),
                cond = self.raw.cond(),
                Rd = reg_string(rd),
                Rm = reg_string(self.raw.rm()),
                Rs = reg_string(self.raw.rs()),
            )
        }
    }

    fn sign_mark(&self) -> &str {
        if self.raw.u_flag() {
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
            mnem = if self.raw.accumulate_flag() {
                "mlal"
            } else {
                "mull"
            },
            S = self.set_cond_mark(),
            cond = self.raw.cond(),
            RdLo = reg_string(self.raw.rd_lo()),
            RdHi = reg_string(self.raw.rd_hi()),
            Rm = reg_string(self.raw.rm()),
            Rs = reg_string(self.raw.rs()),
        )
    }

    fn fmt_ldr_str_hs_imm_offset(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transfer_type = self.raw.halfword_data_transfer_type();
        write!(
            f,
            "{mnem}{type}{cond}\t{Rd}, ",
            mnem = if self.raw.load_flag() { "ldr" } else { "str" },
            cond = self.raw.cond(),
            type = transfer_type,
            Rd = reg_string(self.raw.bit_range(12..16) as usize),
        )?;
        self.fmt_rn_offset(
            f,
            self.raw.ldr_str_hs_imm_offset(),
            self.raw.bit_range(16..20) as usize,
        )
    }

    fn fmt_ldr_str_hs_reg_offset(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let transfer_type = self.raw.halfword_data_transfer_type();
        write!(
            f,
            "{mnem}{type}{cond}\t{Rd}, ",
            mnem = if self.raw.load_flag() { "ldr" } else { "str" },
            cond = self.raw.cond(),
            type = transfer_type,
            Rd = reg_string(self.raw.bit_range(12..16) as usize),
        )?;
        self.fmt_rn_offset(
            f,
            self.raw.ldr_str_hs_reg_offset(),
            self.raw.bit_range(16..20) as usize,
        )
    }

    fn fmt_swi(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "swi{cond}\t#{comm:#x}",
            cond = self.raw.cond(),
            comm = self.raw.swi_comment()
        )
    }

    fn fmt_swp(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "swp{B}{cond}\t{Rd}, {Rm}, [{Rn}]",
            B = if self.raw.transfer_size() == 1 {
                "b"
            } else {
                ""
            },
            cond = self.raw.cond(),
            Rd = reg_string(self.raw.bit_range(12..16) as usize),
            Rm = reg_string(self.raw.rm()),
            Rn = reg_string(self.raw.bit_range(16..20) as usize),
        )
    }
}

impl fmt::Display for ArmInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArmFormat::*;
        match self.fmt {
            BranchExchange => self.fmt_bx(f),
            BranchLink => self.fmt_branch(f),
            DataProcessing => self.fmt_data_processing(f),
            SingleDataTransfer => self.fmt_ldr_str(f),
            BlockDataTransfer => self.fmt_ldm_stm(f),
            MoveFromStatus => self.fmt_mrs(f),
            MoveToStatus => self.fmt_msr(f),
            MoveToFlags => self.fmt_msr_flags(f),
            Multiply => self.fmt_mul_mla(f),
            MultiplyLong => self.fmt_mull_mlal(f),
            HalfwordDataTransferImmediateOffset => self.fmt_ldr_str_hs_imm_offset(f),
            HalfwordDataTransferRegOffset => self.fmt_ldr_str_hs_reg_offset(f),
            SoftwareInterrupt => self.fmt_swi(f),
            SingleDataSwap => self.fmt_swp(f),
            Undefined => write!(f, "<Undefined>"),
        }
    }
}
