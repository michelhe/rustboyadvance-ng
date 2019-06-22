use super::super::reg_string;
use super::arm_isa::{
    ArmCond, ArmInstruction, ArmInstructionFormat, ArmInstructionShiftValue, ArmOpCode, ArmShift,
    ArmShiftType,
};
use std::fmt;

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

fn is_shift(shift: &ArmShift) -> bool {
    if let ArmShift::ImmediateShift(val, typ) = shift {
        return !(*val == 0 && *typ == ArmShiftType::LSL);
    }
    true
}

impl ArmInstruction {
    fn make_shifted_reg_string(&self, reg: usize, shift: ArmShift) -> String {
        let reg = reg_string(reg).to_string();
        if !is_shift(&shift) {
            return reg;
        }

        match shift {
            ArmShift::ImmediateShift(imm, typ) => format!("{}, {} #{}", reg, typ, imm),
            ArmShift::RegisterShift(rs, typ) => format!("{}, {} {}", reg, typ, reg_string(rs)),
        }
    }

    fn fmt_bx(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "bx\t{Rn}", Rn = reg_string(self.rn()))
    }

    fn fmt_branch(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "b{link}{cond}\t{ofs:#x}",
            link = if self.is_linked_branch() { "l" } else { "" },
            cond = self.cond,
            ofs = self.pc.wrapping_add(self.branch_offset() as u32) as u32
        )
    }

    fn fmt_data_processing(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ArmOpCode::*;

        let opcode = self.opcode().unwrap();
        let rd = reg_string(self.rd());

        match opcode {
            // <opcode>{cond}{S} Rd,<Op2>
            MOV | MVN => write!(
                f,
                "{opcode}{cond}{S}\t{Rd}",
                opcode = opcode,
                cond = self.cond,
                S = if self.is_set_cond() { "s" } else { "" },
                Rd = rd
            ),
            // <opcode>{cond}{S} Rd,Rn,<Op2>
            _ => write!(
                f,
                "{opcode}{cond}\t{Rd}, {Rn}",
                opcode = opcode,
                cond = self.cond,
                Rd = rd,
                Rn = reg_string(self.rn())
            ),
        }?;

        let operand2 = self.operand2();
        match operand2 {
            ArmInstructionShiftValue::RotatedImmediate(_, _) => {
                write!(f, ", #{:#x}", operand2.decode_rotated_immediate().unwrap())
            }
            ArmInstructionShiftValue::ShiftedRegister(reg, shift) => {
                write!(f, ", {}", self.make_shifted_reg_string(reg, shift))
            }
            _ => write!(f, "RegisterNotImpl"),
        }
    }

    /// <LDR|STR>{cond}{B}{T} Rd,<Address>
    fn fmt_ldr_str(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{mnem}{B}{cond}{T}\t{Rd}, [{Rn}",
            mnem = if self.is_load() { "ldr" } else { "str" },
            B = if self.transfer_size() == 1 { "b" } else { "" },
            cond = self.cond,
            T = if !self.is_pre_indexing() && self.is_write_back() {
                "t"
            } else {
                ""
            },
            Rd = reg_string(self.rd()),
            Rn = reg_string(self.rn())
        )?;

        let offset = self.offset();
        let auto_incremenet_mark = if self.is_write_back() { "!" } else { "" };
        let sign_mark = if self.is_ofs_added() { '+' } else { '-' };

        let ofs_string = match offset {
            ArmInstructionShiftValue::ImmediateValue(value) => format!("#{:+}", value),
            ArmInstructionShiftValue::ShiftedRegister(reg, shift) => {
                format!("{}{}", sign_mark, self.make_shifted_reg_string(reg, shift))
            }
            _ => panic!("bad barrel shifter"),
        };

        if self.is_pre_indexing() {
            write!(f, ", {}]{}", ofs_string, auto_incremenet_mark)
        } else {
            write!(f, "], {}", ofs_string)
        }
    }

    /// <LDM|STM>{cond}<FD|ED|FA|EA|IA|IB|DA|DB> Rn{!},<Rlist>{^}
    fn fmt_ldm_stm(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{mnem}{inc_dec}{pre_post}{cond}\t{Rn}{auto_inc}, {{",
            mnem = if self.is_load() { "ldm" } else { "stm" },
            inc_dec = if self.is_ofs_added() { 'i' } else { 'd' },
            pre_post = if self.is_pre_indexing() { 'b' } else { 'a' },
            cond = self.cond,
            Rn = reg_string(self.rn()),
            auto_inc = if self.is_write_back() { "!" } else { "" }
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
    /// MRS{cond} Rd,<psr>
    fn fmt_mrs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "mrs{cond}\t{Rd}, {psr}",
            cond = self.cond,
            Rd = reg_string(self.rd()),
            psr = if self.is_spsr() { "SPSR" } else { "CPSR" }
        )
    }

    /// MSR - transfer register contents to PSR
    /// MSR{cond} <psr>,Rm
    fn fmt_msr_reg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "msr{cond}\t{psr}, {Rm}",
            cond = self.cond,
            psr = if self.is_spsr() { "SPSR" } else { "CPSR" },
            Rm = reg_string(self.rm()),

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
            _ => write!(f, "({:?})", self),
        }
    }
}
