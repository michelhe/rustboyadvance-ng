/// The program status register
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bit::BitIndex;
use crate::num::FromPrimitive;

use super::{CpuMode, CpuState};

use colored::*;

impl From<CpuState> for bool {
    fn from(state: CpuState) -> bool {
        match state {
            CpuState::ARM => false,
            CpuState::THUMB => true,
        }
    }
}

impl From<bool> for CpuState {
    fn from(flag: bool) -> CpuState {
        if flag {
            CpuState::THUMB
        } else {
            CpuState::ARM
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct RegPSR {
    raw: u32,
}

const RESERVED_BIT_MASK: u32 = 0x0fffff00;
fn clear_reserved(n: u32) -> u32 {
    n & !RESERVED_BIT_MASK
}

impl RegPSR {
    pub const FLAG_BITMASK: u32 = 0xf000_0000;

    pub fn new(u: u32) -> RegPSR {
        RegPSR {
            raw: clear_reserved(u),
        }
    }

    pub fn get(&self) -> u32 {
        self.raw
    }

    pub fn set(&mut self, psr: u32) {
        self.raw = clear_reserved(psr);
    }

    pub fn set_flag_bits(&mut self, value: u32) {
        self.raw &= !Self::FLAG_BITMASK;
        self.raw |= Self::FLAG_BITMASK & value;
    }

    pub fn state(&self) -> CpuState {
        self.raw.bit(5).into()
    }

    pub fn set_state(&mut self, state: CpuState) {
        self.raw.set_bit(5, state.into());
    }

    pub fn mode(&self) -> CpuMode {
        CpuMode::from_u32(self.raw.bit_range(0..5)).unwrap()
    }

    pub fn set_mode(&mut self, mode: CpuMode) {
        self.raw.set_bit_range(0..5, (mode as u32) & 0b1_1111);
    }

    pub fn irq_disabled(&self) -> bool {
        self.raw.bit(7)
    }

    pub fn set_irq_disabled(&mut self, disabled: bool) {
        self.raw.set_bit(7, disabled);
    }

    pub fn fiq_disabled(&self) -> bool {
        self.raw.bit(6)
    }

    pub fn set_fiq_disabled(&mut self, disabled: bool) {
        self.raw.set_bit(6, disabled);
    }

    #[allow(non_snake_case)]
    pub fn N(&self) -> bool {
        self.raw.bit(31)
    }

    #[allow(non_snake_case)]
    pub fn set_N(&mut self, flag: bool) {
        self.raw.set_bit(31, flag);
    }

    #[allow(non_snake_case)]
    pub fn Z(&self) -> bool {
        self.raw.bit(30)
    }

    #[allow(non_snake_case)]
    pub fn set_Z(&mut self, flag: bool) {
        self.raw.set_bit(30, flag);
    }

    #[allow(non_snake_case)]
    pub fn C(&self) -> bool {
        self.raw.bit(29)
    }

    #[allow(non_snake_case)]
    pub fn set_C(&mut self, flag: bool) {
        self.raw.set_bit(29, flag);
    }

    #[allow(non_snake_case)]
    pub fn V(&self) -> bool {
        self.raw.bit(28)
    }

    #[allow(non_snake_case)]
    pub fn set_V(&mut self, flag: bool) {
        self.raw.set_bit(28, flag);
    }
}

impl fmt::Display for RegPSR {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let disabled_string = |disabled: bool| -> ColoredString {
            if disabled {
                "disabled".bright_red()
            } else {
                "enabled".bright_green()
            }
        };
        write!(
            f,
            "{{ [{raw:#010x}] mode: {mode}, state: {state}, irq: {irq}, fiq: {fiq}, condition_flags: (N={N} Z={Z} C={C} V={V}) }}",
            raw = self.raw,
            mode = self.mode(),
            state = self.state(),
            irq = disabled_string(self.irq_disabled()),
            fiq = disabled_string(self.irq_disabled()),
            N = self.N() as u8,
            Z = self.Z() as u8,
            C = self.C() as u8,
            V = self.V() as u8,
            )
    }
}
