use std::fmt;

use crate::bit::BitIndex;

use super::*;
use crate::arm7tdmi::*;

use super::ThumbDecodeHelper;

pub(super) mod consts {
    pub(super) mod flags {
        pub const FLAG_H1: usize = 7;
        pub const FLAG_H2: usize = 6;
        pub const FLAG_R: usize = 8;
        pub const FLAG_LOW_OFFSET: usize = 11;
        pub const FLAG_SP: usize = 11;
        pub const FLAG_SIGN_EXTEND: usize = 10;
        pub const FLAG_HALFWORD: usize = 11;
    }
}

impl ThumbInstruction {
    fn fmt_thumb_move_shifted_reg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, {Rs}, #{Offset5}",
            op = self.raw.format1_op(),
            Rd = reg_string(self.raw & 0b111),
            Rs = reg_string(self.raw.rs()),
            Offset5 = self.raw.offset5()
        )
    }

    fn fmt_thumb_data_process_imm(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, #{Offset8:#x}",
            op = self.raw.format3_op(),
            Rd = reg_string(self.raw.bit_range(8..11)),
            Offset8 = self.raw & 0xff
        )
    }

    fn fmt_thumb_alu_ops(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, {Rs}",
            op = self.raw.format4_alu_op(),
            Rd = reg_string(self.raw & 0b111),
            Rs = reg_string(self.raw.rs())
        )
    }

    fn fmt_thumb_high_reg_op_or_bx(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = self.raw.format5_op();
        let dst_reg = if self.raw.flag(consts::flags::FLAG_H1) {
            self.raw & 0b111 + 8
        } else {
            self.raw & 0b111
        };
        let src_reg = if self.raw.flag(consts::flags::FLAG_H2) {
            self.raw.rs() + 8
        } else {
            self.raw.rs()
        };

        write!(f, "{}\t", op)?;
        match op {
            OpFormat5::BX => write!(f, "{}", reg_string(src_reg)),
            _ => write!(
                f,
                "{dst}, {src}",
                dst = reg_string(dst_reg),
                src = reg_string(src_reg)
            ),
        }
    }

    fn fmt_thumb_ldr_pc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ldr\t{Rd}, [pc, #{Imm:#x}] ; = #{effective:#x}",
            Rd = reg_string(self.raw.bit_range(8..11)),
            Imm = self.raw.word8(),
            effective = (self.pc + 4 & !0b10) + (self.raw.word8() as Addr)
        )
    }

    fn fmt_thumb_ldr_str_reg_offset(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}{b}\t{Rd}, [{Rb}, {Ro}]",
            op = if self.raw.is_load() { "ldr" } else { "str" },
            b = if self.raw.bit(10) { "b" } else { "" },
            Rd = reg_string(self.raw & 0b111),
            Rb = reg_string(self.raw.rb()),
            Ro = reg_string(self.raw.ro()),
        )
    }

    fn fmt_thumb_ldr_str_shb(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, [{Rb}, {Ro}]",
            op = {
                match (
                    self.raw.flag(consts::flags::FLAG_SIGN_EXTEND),
                    self.raw.flag(consts::flags::FLAG_HALFWORD),
                ) {
                    (false, false) => "strh",
                    (false, true) => "ldrh",
                    (true, false) => "ldsb",
                    (true, true) => "ldsh",
                }
            },
            Rd = reg_string(self.raw & 0b111),
            Rb = reg_string(self.raw.rb()),
            Ro = reg_string(self.raw.ro()),
        )
    }

    fn fmt_thumb_ldr_str_imm_offset(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let is_transferring_bytes = self.raw.bit(12);
        write!(
            f,
            "{op}{b}\t{Rd}, [{Rb}, #{imm:#x}]",
            op = if self.raw.is_load() { "ldr" } else { "str" },
            b = if is_transferring_bytes { "b" } else { "" },
            Rd = reg_string(self.raw & 0b111),
            Rb = reg_string(self.raw.rb()),
            imm = {
                let offset5 = self.raw.offset5();
                if is_transferring_bytes {
                    offset5
                } else {
                    (offset5 << 3) >> 1
                }
            },
        )
    }

    fn fmt_thumb_ldr_str_halfword(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, [{Rb}, #{imm:#x}]",
            op = if self.raw.is_load() { "ldrh" } else { "strh" },
            Rd = reg_string(self.raw & 0b111),
            Rb = reg_string(self.raw.rb()),
            imm = self.raw.offset5() << 1
        )
    }

    fn fmt_thumb_ldr_str_sp(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, [sp, #{Imm:#x}]",
            op = if self.raw.is_load() { "ldr" } else { "str" },
            Rd = reg_string(self.raw.bit_range(8..11)),
            Imm = self.raw.word8(),
        )
    }

    fn fmt_thumb_load_address(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "add\t{Rd}, {r}, #{Imm:#x}",
            Rd = reg_string(self.raw.bit_range(8..11)),
            r = if self.raw.flag(consts::flags::FLAG_SP) {
                "sp"
            } else {
                "pc"
            },
            Imm = self.raw.word8(),
        )
    }

    fn fmt_thumb_add_sub(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operand = if self.raw.is_immediate_operand() {
            format!("#{:x}", self.raw.bit_range(6..9))
        } else {
            String::from(reg_string(self.raw.rn()))
        };

        write!(
            f,
            "{op}\t{Rd}, {Rs}, {operand}",
            op = if self.raw.is_subtract() { "sub" } else { "add" },
            Rd = reg_string(self.raw & 0b111),
            Rs = reg_string(self.raw.rs()),
            operand = operand
        )
    }

    fn fmt_thumb_add_sp(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "add\tsp, #{imm:x}", imm = self.raw.sword7())
    }

    fn fmt_register_list(&self, f: &mut fmt::Formatter<'_>, rlist: u8) -> fmt::Result {
        let mut has_first = false;
        for i in 0..8 {
            if rlist.bit(i) {
                if has_first {
                    write!(f, ", {}", reg_string(i))?;
                } else {
                    has_first = true;
                    write!(f, "{}", reg_string(i))?;
                }
            }
        }
        Ok(())
    }

    fn fmt_thumb_push_pop(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\t{{", if self.raw.is_load() { "pop" } else { "push" })?;
        let rlist = self.raw.register_list();
        self.fmt_register_list(f, rlist)?;
        if self.raw.flag(consts::flags::FLAG_R) {
            let r = if self.raw.is_load() { "pc" } else { "lr" };
            if rlist != 0 {
                write!(f, ", {}", r)?;
            } else {
                write!(f, "{}", r)?;
            }
        }
        write!(f, "}}")
    }

    fn fmt_thumb_ldm_stm(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rb}!, {{",
            op = if self.raw.is_load() { "ldm" } else { "stm" },
            Rb = reg_string(self.raw.rb()),
        )?;
        self.fmt_register_list(f, self.raw.register_list())?;
        write!(f, "}}")
    }

    fn fmt_thumb_branch_with_cond(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "b{cond}\t{addr:#x}",
            cond = self.raw.cond(),
            addr = {
                let offset = self.raw.bcond_offset();
                (self.pc as i32 + 4).wrapping_add(offset) as Addr
            }
        )
    }

    fn fmt_thumb_swi(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "swi\t{value:#x}", value = self.raw & 0xff,)
    }

    fn fmt_thumb_branch(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "b\t{addr:#x}",
            addr = {
                let offset = (self.raw.offset11() << 21) >> 20;
                (self.pc as i32 + 4).wrapping_add(offset) as Addr
            }
        )
    }

    fn fmt_thumb_branch_long_with_link(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bl\t#0x{:08x}", {
            let offset11 = self.raw.offset11();
            if self.raw.flag(consts::flags::FLAG_LOW_OFFSET) {
                (offset11 << 1) as i32
            } else {
                ((offset11 << 21) >> 9) as i32
            }
        })
    }
}

#[cfg(feature = "debugger")]
impl fmt::Display for ThumbInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.fmt {
            ThumbFormat::MoveShiftedReg => self.fmt_thumb_move_shifted_reg(f),
            ThumbFormat::AddSub => self.fmt_thumb_add_sub(f),
            ThumbFormat::DataProcessImm => self.fmt_thumb_data_process_imm(f),
            ThumbFormat::AluOps => self.fmt_thumb_alu_ops(f),
            ThumbFormat::HiRegOpOrBranchExchange => self.fmt_thumb_high_reg_op_or_bx(f),
            ThumbFormat::LdrPc => self.fmt_thumb_ldr_pc(f),
            ThumbFormat::LdrStrRegOffset => self.fmt_thumb_ldr_str_reg_offset(f),
            ThumbFormat::LdrStrSHB => self.fmt_thumb_ldr_str_shb(f),
            ThumbFormat::LdrStrImmOffset => self.fmt_thumb_ldr_str_imm_offset(f),
            ThumbFormat::LdrStrHalfWord => self.fmt_thumb_ldr_str_halfword(f),
            ThumbFormat::LdrStrSp => self.fmt_thumb_ldr_str_sp(f),
            ThumbFormat::LoadAddress => self.fmt_thumb_load_address(f),
            ThumbFormat::AddSp => self.fmt_thumb_add_sp(f),
            ThumbFormat::PushPop => self.fmt_thumb_push_pop(f),
            ThumbFormat::LdmStm => self.fmt_thumb_ldm_stm(f),
            ThumbFormat::BranchConditional => self.fmt_thumb_branch_with_cond(f),
            ThumbFormat::Swi => self.fmt_thumb_swi(f),
            ThumbFormat::Branch => self.fmt_thumb_branch(f),
            ThumbFormat::BranchLongWithLink => self.fmt_thumb_branch_long_with_link(f),
            ThumbFormat::Undefined => write!(f, "<Undefined>"),
        }
    }
}

impl fmt::Display for OpFormat3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpFormat3::MOV => write!(f, "mov"),
            OpFormat3::CMP => write!(f, "cmp"),
            OpFormat3::ADD => write!(f, "add"),
            OpFormat3::SUB => write!(f, "sub"),
        }
    }
}

impl fmt::Display for OpFormat5 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpFormat5::ADD => write!(f, "add"),
            OpFormat5::CMP => write!(f, "cmp"),
            OpFormat5::MOV => write!(f, "mov"),
            OpFormat5::BX => write!(f, "bx"),
        }
    }
}

impl fmt::Display for ThumbAluOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ThumbAluOps::*;
        match self {
            AND => write!(f, "and"),
            EOR => write!(f, "eor"),
            LSL => write!(f, "lsl"),
            LSR => write!(f, "lsr"),
            ASR => write!(f, "asr"),
            ADC => write!(f, "adc"),
            SBC => write!(f, "sbc"),
            ROR => write!(f, "ror"),
            TST => write!(f, "tst"),
            NEG => write!(f, "neg"),
            CMP => write!(f, "cmp"),
            CMN => write!(f, "cmn"),
            ORR => write!(f, "orr"),
            MUL => write!(f, "mul"),
            BIC => write!(f, "bic"),
            MVN => write!(f, "mvn"),
        }
    }
}
