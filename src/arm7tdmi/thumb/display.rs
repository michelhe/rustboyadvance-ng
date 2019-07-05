use std::fmt;

use crate::bit::BitIndex;

use super::*;
use crate::arm7tdmi::*;

impl ThumbInstruction {
    fn fmt_thumb_move_shifted_reg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, {Rs}, #{Offset5}",
            op = self.format1_op(),
            Rd = reg_string(self.rd()),
            Rs = reg_string(self.rs()),
            Offset5 = self.offset5()
        )
    }

    fn fmt_thumb_data_process_imm(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, #{Offset8:#x}",
            op = self.format3_op(),
            Rd = reg_string(self.rd()),
            Offset8 = self.offset8()
        )
    }

    fn fmt_thumb_mul(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "mul\t{Rd}, {Rs}",
            Rd = reg_string(self.rd()),
            Rs = reg_string(self.rs())
        )
    }

    fn fmt_thumb_alu_ops(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, {Rs}",
            op = self.alu_opcode(),
            Rd = reg_string(self.rd()),
            Rs = reg_string(self.rs())
        )
    }

    fn fmt_thumb_high_reg_op_or_bx(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let op = self.format5_op();
        let dst_reg = if self.flag(ThumbInstruction::FLAG_H1) {
            self.rd() + 8
        } else {
            self.rd()
        };
        let src_reg = if self.flag(ThumbInstruction::FLAG_H2) {
            self.rs() + 8
        } else {
            self.rs()
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

    fn fmt_thumb_ldr_pc(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ldr\t{Rd}, [pc, #{Imm:#x}] ; = #{effective:#x}",
            Rd = reg_string(self.rd()),
            Imm = self.word8(),
            effective = (self.pc + 4 & !0b10) + (self.word8() as Addr)
        )
    }

    fn fmt_thumb_ldr_str_reg_offset(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}{b}\t{Rd}, [{Rb}, {Ro}]",
            op = if self.is_load() { "ldr" } else { "str" },
            b = if self.is_transfering_bytes() { "b" } else { "" },
            Rd = reg_string(self.rd()),
            Rb = reg_string(self.rb()),
            Ro = reg_string(self.ro()),
        )
    }

    fn fmt_thumb_ldr_str_halfword(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, [{Rb}, #{imm:#x}]",
            op = if self.is_load() { "ldrh" } else { "strh" },
            Rd = reg_string(self.rd()),
            Rb = reg_string(self.rb()),
            imm = self.offset5() << 1
        )
    }

    fn fmt_thumb_ldr_str_sp(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{op}\t{Rd}, [sp, #{Imm:#x}]",
            op = if self.is_load() { "ldr" } else { "str" },
            Rd = reg_string(self.rd()),
            Imm = self.word8(),
        )
    }

    fn fmt_thumb_add_sub(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let operand = if self.is_immediate_operand() {
            format!("#{:x}", self.raw.bit_range(6..9))
        } else {
            String::from(reg_string(self.rn()))
        };

        write!(
            f,
            "{op}\t{Rd}, [{Rs}, {operand}]",
            op = if self.is_subtract() { "sub" } else { "add" },
            Rd = reg_string(self.rd()),
            Rs = reg_string(self.rs()),
            operand = operand
        )
    }

    fn fmt_thumb_add_sp(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "add\tsp, #{imm:x}", imm = self.sword7())
    }

    fn fmt_thumb_push_pop(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}\t{{", if self.is_load() { "pop" } else { "push" })?;
        let mut register_list = self.register_list().into_iter();
        let mut has_reg = false;
        if let Some(reg) = register_list.next() {
            write!(f, "{}", reg_string(reg))?;
            has_reg = true;
        }
        for reg in register_list {
            has_reg = true;
            write!(f, ", {}", reg_string(reg))?;
        }
        if self.flag(ThumbInstruction::FLAG_R) {
            let r = if self.is_load() { "pc" } else { "lr" };
            if has_reg {
                write!(f, ", {}", r)?;
            } else {
                write!(f, "{}", r)?;
            }
        }
        write!(f, "}}")
    }

    fn fmt_thumb_branch_with_cond(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "b{cond}\t{addr:#x}",
            cond = self.cond(),
            addr = {
                let offset = ((self.offset8() as i8) << 1) as i32;
                (self.pc as i32 + 4).wrapping_add(offset) as Addr
            }
        )
    }

    fn fmt_thumb_branch_long_with_link(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "bl\t#0x{:08x}", {
            let offset11 = self.offset11();
            if self.flag(ThumbInstruction::FLAG_LOW_OFFSET) {
                (offset11 << 1) as i32
            } else {
                ((offset11 << 21) >> 9) as i32
            }
        })
    }
}

impl fmt::Display for ThumbInstruction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.fmt {
            ThumbFormat::MoveShiftedReg => self.fmt_thumb_move_shifted_reg(f),
            ThumbFormat::AddSub => self.fmt_thumb_add_sub(f),
            ThumbFormat::DataProcessImm => self.fmt_thumb_data_process_imm(f),
            ThumbFormat::Mul => self.fmt_thumb_mul(f),
            ThumbFormat::AluOps => self.fmt_thumb_alu_ops(f),
            ThumbFormat::HiRegOpOrBranchExchange => self.fmt_thumb_high_reg_op_or_bx(f),
            ThumbFormat::LdrPc => self.fmt_thumb_ldr_pc(f),
            ThumbFormat::LdrStrRegOffset => self.fmt_thumb_ldr_str_reg_offset(f),
            ThumbFormat::LdrStrHalfWord => self.fmt_thumb_ldr_str_halfword(f),
            ThumbFormat::LdrStrSp => self.fmt_thumb_ldr_str_sp(f),
            ThumbFormat::AddSp => self.fmt_thumb_add_sp(f),
            ThumbFormat::PushPop => self.fmt_thumb_push_pop(f),
            ThumbFormat::BranchConditional => self.fmt_thumb_branch_with_cond(f),
            ThumbFormat::BranchLongWithLink => self.fmt_thumb_branch_long_with_link(f),
            _ => write!(f, "({:?})", self),
        }
    }
}

impl fmt::Display for OpFormat3 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OpFormat3::MOV => write!(f, "mov"),
            OpFormat3::CMP => write!(f, "cmp"),
            OpFormat3::ADD => write!(f, "add"),
            OpFormat3::SUB => write!(f, "sub"),
        }
    }
}

impl fmt::Display for OpFormat5 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OpFormat5::ADD => write!(f, "add"),
            OpFormat5::CMP => write!(f, "cmp"),
            OpFormat5::MOV => write!(f, "mov"),
            OpFormat5::BX => write!(f, "bx"),
        }
    }
}
