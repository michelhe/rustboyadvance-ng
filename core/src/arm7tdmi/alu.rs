use bit::BitIndex;

use super::memory::MemoryInterface;
use super::{Core, REG_PC};

#[derive(Debug, Primitive, Eq, PartialEq)]
pub enum AluOpCode {
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

impl AluOpCode {
    pub fn is_setting_flags(&self) -> bool {
        use AluOpCode::*;
        match self {
            TST | TEQ | CMP | CMN => true,
            _ => false,
        }
    }

    pub fn is_logical(&self) -> bool {
        use AluOpCode::*;
        match self {
            MOV | MVN | ORR | EOR | AND | BIC | TST | TEQ => true,
            _ => false,
        }
    }
    pub fn is_arithmetic(&self) -> bool {
        use AluOpCode::*;
        match self {
            ADD | ADC | SUB | SBC | RSB | RSC | CMP | CMN => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Primitive, Copy, Clone)]
pub enum BarrelShiftOpCode {
    LSL = 0,
    LSR = 1,
    ASR = 2,
    ROR = 3,
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ShiftRegisterBy {
    ByAmount(u32),
    ByRegister(usize),
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct ShiftedRegister {
    pub reg: usize,
    pub shift_by: ShiftRegisterBy,
    pub bs_op: BarrelShiftOpCode,
    pub added: Option<bool>,
}

impl ShiftedRegister {
    pub fn is_shifted_by_reg(&self) -> bool {
        match self.shift_by {
            ShiftRegisterBy::ByRegister(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum BarrelShifterValue {
    ImmediateValue(u32),
    RotatedImmediate(u32, u32),
    ShiftedRegister(ShiftedRegister),
}

impl BarrelShifterValue {
    /// Decode operand2 as an immediate value
    pub fn decode_rotated_immediate(&self) -> Option<u32> {
        if let BarrelShifterValue::RotatedImmediate(immediate, rotate) = self {
            return Some(immediate.rotate_right(*rotate) as u32);
        }
        None
    }
    pub fn shifted_register(
        reg: usize,
        shift_by: ShiftRegisterBy,
        bs_op: BarrelShiftOpCode,
        added: Option<bool>,
    ) -> BarrelShifterValue {
        let shft_reg = ShiftedRegister {
            reg,
            shift_by,
            bs_op,
            added,
        };
        BarrelShifterValue::ShiftedRegister(shft_reg)
    }
}

impl<I: MemoryInterface> Core<I> {
    pub fn lsl(&mut self, val: u32, amount: u32, carry: &mut bool) -> u32 {
        match amount {
            0 => val,
            x if x < 32 => {
                *carry = val.wrapping_shr(32 - x) & 1 == 1;
                val << x
            }
            32 => {
                *carry = val & 1 == 1;
                0
            }
            _ => {
                *carry = false;
                0
            }
        }
    }

    pub fn lsr(&mut self, val: u32, amount: u32, carry: &mut bool, immediate: bool) -> u32 {
        if amount != 0 {
            match amount {
                x if x < 32 => {
                    *carry = (val >> (amount - 1) & 1) == 1;
                    val >> amount
                }
                32 => {
                    *carry = val.bit(31);
                    0
                }
                _ => {
                    *carry = false;
                    0
                }
            }
        } else if immediate {
            *carry = val.bit(31);
            0
        } else {
            val
        }
    }

    pub fn asr(&mut self, val: u32, amount: u32, carry: &mut bool, immediate: bool) -> u32 {
        let amount = if immediate && amount == 0 { 32 } else { amount };
        match amount {
            0 => val,
            x if x < 32 => {
                *carry = val.wrapping_shr(amount - 1) & 1 == 1;
                (val as i32).wrapping_shr(amount) as u32
            }
            _ => {
                let bit31 = val.bit(31);
                *carry = bit31;
                if bit31 {
                    0xffffffff
                } else {
                    0
                }
            }
        }
    }

    pub fn rrx(&mut self, val: u32, carry: &mut bool) -> u32 {
        let old_c = *carry as i32;
        *carry = val & 0b1 != 0;
        (((val as u32) >> 1) as i32 | (old_c << 31)) as u32
    }

    pub fn ror(
        &mut self,
        val: u32,
        amount: u32,
        carry: &mut bool,
        immediate: bool,
        rrx: bool,
    ) -> u32 {
        match amount {
            0 => {
                if immediate & rrx {
                    self.rrx(val, carry)
                } else {
                    val
                }
            }
            _ => {
                let amount = amount % 32;
                let val = if amount != 0 {
                    val.rotate_right(amount)
                } else {
                    val
                };
                *carry = (val as u32).bit(31);
                val
            }
        }
    }

    /// Performs a generic barrel shifter operation
    #[inline]
    pub fn barrel_shift_op(
        &mut self,
        shift: BarrelShiftOpCode,
        val: u32,
        amount: u32,
        carry: &mut bool,
        immediate: bool,
    ) -> u32 {
        //
        // From GBATEK:
        // Zero Shift Amount (Shift Register by Immediate, with Immediate=0)
        //  LSL#0: No shift performed, ie. directly Op2=Rm, the C flag is NOT affected.
        //  LSR#0: Interpreted as LSR#32, ie. Op2 becomes zero, C becomes Bit 31 of Rm.
        //  ASR#0: Interpreted as ASR#32, ie. Op2 and C are filled by Bit 31 of Rm.
        //  ROR#0: Interpreted as RRX#1 (RCR), like ROR#1, but Op2 Bit 31 set to old C.
        //
        // From ARM7TDMI Datasheet:
        // 1 LSL by 32 has result zero, carry out equal to bit 0 of Rm.
        // 2 LSL by more than 32 has result zero, carry out zero.
        // 3 LSR by 32 has result zero, carry out equal to bit 31 of Rm.
        // 4 LSR by more than 32 has result zero, carry out zero.
        // 5 ASR by 32 or more has result filled with and carry out equal to bit 31 of Rm.
        // 6 ROR by 32 has result equal to Rm, carry out equal to bit 31 of Rm.
        // 7 ROR by n where n is greater than 32 will give the same result and carry out
        //   as ROR by n-32; therefore repeatedly subtract 32 from n until the amount is
        //   in the range 1 to 32 and see above.
        //
        match shift {
            BarrelShiftOpCode::LSL => self.lsl(val, amount, carry),
            BarrelShiftOpCode::LSR => self.lsr(val, amount, carry, immediate),
            BarrelShiftOpCode::ASR => self.asr(val, amount, carry, immediate),
            BarrelShiftOpCode::ROR => self.ror(val, amount, carry, immediate, true),
        }
    }

    #[inline]
    pub fn shift_by_register(
        &mut self,
        bs_op: BarrelShiftOpCode,
        reg: usize,
        rs: usize,
        carry: &mut bool,
    ) -> u32 {
        let mut val = self.get_reg(reg);
        if reg == REG_PC {
            val += 4; // PC prefetching
        }
        let amount = self.get_reg(rs) & 0xff;
        self.barrel_shift_op(bs_op, val, amount, carry, false)
    }

    pub fn register_shift_const<const BS_OP: u8, const SHIFT_BY_REG: bool>(
        &mut self,
        offset: u32,
        reg: usize,
        carry: &mut bool,
    ) -> u32 {
        let op = match BS_OP {
            0 => BarrelShiftOpCode::LSL,
            1 => BarrelShiftOpCode::LSR,
            2 => BarrelShiftOpCode::ASR,
            3 => BarrelShiftOpCode::ROR,
            _ => unsafe { std::hint::unreachable_unchecked() },
        };
        if SHIFT_BY_REG {
            let rs = offset.bit_range(8..12) as usize;
            self.shift_by_register(op, reg, rs, carry)
        } else {
            let amount = offset.bit_range(7..12) as u32;
            self.barrel_shift_op(op, self.get_reg(reg), amount, carry, true)
        }
    }

    pub fn register_shift(&mut self, shift: &ShiftedRegister, carry: &mut bool) -> u32 {
        match shift.shift_by {
            ShiftRegisterBy::ByAmount(amount) => {
                let result =
                    self.barrel_shift_op(shift.bs_op, self.get_reg(shift.reg), amount, carry, true);
                result
            }
            ShiftRegisterBy::ByRegister(rs) => {
                self.shift_by_register(shift.bs_op, shift.reg, rs, carry)
            }
        }
    }

    pub fn get_barrel_shifted_value(&mut self, sval: &BarrelShifterValue, carry: &mut bool) -> u32 {
        // TODO decide if error handling or panic here
        match sval {
            BarrelShifterValue::ImmediateValue(offset) => *offset as u32,
            BarrelShifterValue::ShiftedRegister(shifted_reg) => {
                let added = (*shifted_reg).added.unwrap_or(true);
                let abs = self.register_shift(shifted_reg, carry) as u32;
                if added {
                    abs as u32
                } else {
                    (-(abs as i32)) as u32
                }
            }
            _ => panic!("bad barrel shift"),
        }
    }

    pub(super) fn alu_sub_flags(
        &self,
        a: u32,
        b: u32,
        carry: &mut bool,
        overflow: &mut bool,
    ) -> u32 {
        let res = a.wrapping_sub(b);
        *carry = b <= a;
        *overflow = (a as i32).overflowing_sub(b as i32).1;
        res
    }

    pub(super) fn alu_add_flags(
        &self,
        a: u32,
        b: u32,
        carry: &mut bool,
        overflow: &mut bool,
    ) -> u32 {
        let res = a.wrapping_add(b);
        *carry = add_carry_result(a as u64, b as u64);
        *overflow = (a as i32).overflowing_add(b as i32).1;
        res
    }

    pub(super) fn alu_adc_flags(
        &self,
        a: u32,
        b: u32,
        carry: &mut bool,
        overflow: &mut bool,
    ) -> u32 {
        let c = self.cpsr.C() as u64;
        let res = (a as u64) + (b as u64) + c;
        *carry = res > 0xffffffff;
        *overflow = (!(a ^ b) & (b ^ (res as u32))).bit(31);
        res as u32
    }

    pub(super) fn alu_sbc_flags(
        &self,
        a: u32,
        b: u32,
        carry: &mut bool,
        overflow: &mut bool,
    ) -> u32 {
        self.alu_adc_flags(a, !b, carry, overflow)
    }

    pub fn alu_update_flags(&mut self, result: u32, _is_arithmetic: bool, c: bool, v: bool) {
        self.cpsr.set_N((result as i32) < 0);
        self.cpsr.set_Z(result == 0);
        self.cpsr.set_C(c);
        self.cpsr.set_V(v);
    }
}

#[inline]
fn add_carry_result(a: u64, b: u64) -> bool {
    a.wrapping_add(b) > 0xffffffff
}
