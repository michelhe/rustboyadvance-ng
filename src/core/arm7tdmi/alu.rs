use bit::BitIndex;
use num::FromPrimitive;

use super::{Core, CpuError, CpuResult, REG_PC};

#[derive(Debug, Primitive, PartialEq)]
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

#[derive(Debug, PartialEq, Primitive)]
pub enum BarrelShiftOpCode {
    LSL = 0,
    LSR = 1,
    ASR = 2,
    ROR = 3,
}

#[derive(Debug, PartialEq)]
pub enum ShiftedRegister {
    ByAmount(u32, BarrelShiftOpCode),
    ByRegister(usize, BarrelShiftOpCode),
}

impl From<u32> for ShiftedRegister {
    fn from(v: u32) -> ShiftedRegister {
        let typ = BarrelShiftOpCode::from_u8(v.bit_range(5..7) as u8).unwrap();
        if v.bit(4) {
            let rs = v.bit_range(8..12) as usize;
            ShiftedRegister::ByRegister(rs, typ)
        } else {
            let amount = v.bit_range(7..12) as u32;
            ShiftedRegister::ByAmount(amount, typ)
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum BarrelShifterValue {
    ImmediateValue(i32),
    RotatedImmediate(u32, u32),
    ShiftedRegister {
        reg: usize,
        shift: ShiftedRegister,
        added: Option<bool>,
    },
}

impl BarrelShifterValue {
    /// Decode operand2 as an immediate value
    pub fn decode_rotated_immediate(&self) -> Option<i32> {
        if let BarrelShifterValue::RotatedImmediate(immediate, rotate) = self {
            return Some(immediate.rotate_right(*rotate) as i32);
        }
        None
    }
}

impl Core {
    /// Performs a generic barrel shifter operation
    fn barrel_shift(
        &mut self,
        val: i32,
        amount: u32,
        shift: BarrelShiftOpCode,
        immediate: bool,
    ) -> i32 {
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
            BarrelShiftOpCode::LSL => match amount {
                0 => val,
                x if x < 32 => {
                    self.cpsr.set_C(val.wrapping_shr(32 - x) & 1 == 1);
                    val << x
                }
                32 => {
                    self.cpsr.set_C(val & 1 == 1);
                    0
                }
                _ => {
                    self.cpsr.set_C(false);
                    0
                }
            },
            BarrelShiftOpCode::LSR => match amount {
                0 | 32 => {
                    if immediate {
                        self.cpsr.set_C((val as u32).bit(31));
                        0
                    } else {
                        val
                    }
                }
                x if x < 32 => {
                    self.cpsr.set_C(val >> (amount - 1) & 1 == 1);
                    ((val as u32) >> amount) as i32
                }
                _ => {
                    self.cpsr.set_C(false);
                    0
                }
            },
            BarrelShiftOpCode::ASR => match amount {
                0 => {
                    if immediate {
                        let bit31 = (val as u32).bit(31);
                        self.cpsr.set_C(bit31);
                        if bit31 {
                            -1
                        } else {
                            0
                        }
                    } else {
                        val
                    }
                }
                x if x < 32 => {
                    self.cpsr.set_C(val.wrapping_shr(amount - 1) & 1 == 1);
                    val.wrapping_shr(amount)
                }
                _ => {
                    let bit31 = (val as u32).bit(31);
                    self.cpsr.set_C(bit31);
                    if bit31 {
                        -1
                    } else {
                        0
                    }
                }
            },
            BarrelShiftOpCode::ROR => {
                match amount {
                    0 => {
                        if immediate {
                            /* RRX */
                            let old_c = self.cpsr.C() as i32;
                            self.cpsr.set_C(val & 0b1 != 0);
                            ((val as u32) >> 1) as i32 | (old_c << 31)
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
                        self.cpsr.set_C((val as u32).bit(31));
                        val
                    }
                }
            }
        }
    }

    pub fn register_shift(&mut self, reg: usize, shift: ShiftedRegister) -> CpuResult<i32> {
        let val = self.get_reg(reg) as i32;
        match shift {
            ShiftedRegister::ByAmount(amount, shift) => {
                Ok(self.barrel_shift(val, amount, shift, true))
            }
            ShiftedRegister::ByRegister(reg, shift) => {
                if reg != REG_PC {
                    Ok(self.barrel_shift(val, self.get_reg(reg) & 0xff, shift, false))
                } else {
                    Err(CpuError::IllegalInstruction)
                }
            }
        }
    }

    pub fn get_barrel_shifted_value(&mut self, sval: BarrelShifterValue) -> i32 {
        // TODO decide if error handling or panic here
        match sval {
            BarrelShifterValue::ImmediateValue(offset) => offset,
            BarrelShifterValue::ShiftedRegister {
                reg,
                shift,
                added: Some(added),
            } => {
                let abs = self.register_shift(reg, shift).unwrap();
                if added {
                    abs
                } else {
                    -abs
                }
            }
            _ => panic!("bad barrel shift"),
        }
    }

    fn alu_sub_flags(a: i32, b: i32, carry: &mut bool, overflow: &mut bool) -> i32 {
        let res = a.wrapping_sub(b);
        *carry = b <= a;
        let (_, would_overflow) = a.overflowing_sub(b);
        *overflow = would_overflow;
        res
    }

    fn alu_add_flags(a: i32, b: i32, carry: &mut bool, overflow: &mut bool) -> i32 {
        let res = a.wrapping_add(b) as u32;
        *carry = res < a as u32 || res < b as u32;
        let (_, would_overflow) = a.overflowing_add(b);
        *overflow = would_overflow;
        res as i32
    }

    #[allow(non_snake_case)]
    pub fn alu(
        &mut self,
        opcode: AluOpCode,
        op1: i32,
        op2: i32,
        set_cond_flags: bool,
    ) -> Option<i32> {
        use AluOpCode::*;

        let C = self.cpsr.C() as i32;

        let mut carry = self.cpsr.C();
        let mut overflow = self.cpsr.V();

        let result = match opcode {
            AND | TST => op1 & op2,
            EOR | TEQ => op1 ^ op2,
            SUB | CMP => Self::alu_sub_flags(op1, op2, &mut carry, &mut overflow),
            RSB => Self::alu_sub_flags(op2, op1, &mut carry, &mut overflow),
            ADD | CMN => Self::alu_add_flags(op1, op2, &mut carry, &mut overflow),
            ADC => Self::alu_add_flags(op1, op2.wrapping_add(C), &mut carry, &mut overflow),
            SBC => Self::alu_sub_flags(op1, op2, &mut carry, &mut overflow).wrapping_sub(1 - C),
            RSC => Self::alu_sub_flags(op2, op1, &mut carry, &mut overflow).wrapping_sub(1 - C),
            ORR => op1 | op2,
            MOV => op2,
            BIC => op1 & (!op2),
            MVN => !op2,
        };

        if set_cond_flags {
            self.cpsr.set_N(result < 0);
            self.cpsr.set_Z(result == 0);
            self.cpsr.set_C(carry);
            if opcode.is_arithmetic() {
                self.cpsr.set_V(overflow);
            }
        }

        match opcode {
            TST | TEQ | CMP | CMN => None,
            _ => Some(result),
        }
    }
}
