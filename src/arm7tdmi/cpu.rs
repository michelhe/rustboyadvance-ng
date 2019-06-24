use crate::num_traits::FromPrimitive;

use super::arm::*;
use super::sysbus::SysBus;

#[derive(Debug, PartialEq)]
pub enum CpuError {
    IllegalInstruction,
}

pub type CpuResult<T> = Result<T, CpuError>;

#[derive(Debug, PartialEq)]
pub enum CpuState {
    ARM,
    THUMB,
}

#[derive(Debug, Primitive)]
#[repr(u8)]
enum CpuMode {
    User = 0b10000,
    Fiq = 0b10001,
    Irq = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    Undefined = 0b11011,
    System = 0b11111,
}

pub struct CpuModeContext {
    // r8-r14
    banked_gpr: [u32; 7],
    spsr: u32,
}

#[derive(Debug)]
pub struct Core {
    pc: u32,
    // r0-r7
    gpr: [u32; 8],
    cpsr: u32,

    mode: CpuMode,
    state: CpuState,
}

impl Core {
    pub fn new() -> Core {
        Core {
            pc: 0,
            gpr: [0; 8],
            cpsr: 0,
            mode: CpuMode::System,
            state: CpuState::ARM,
        }
    }

    pub fn get_reg(&self, reg_num: usize) -> u32 {
        match reg_num {
            0...7 => self.gpr[reg_num],
            15 => self.pc,
            _ => unimplemented!("TODO banked registers"),
        }
    }

    pub fn set_reg(&mut self, reg_num: usize, val: u32) {
        match reg_num {
            0...7 => self.gpr[reg_num] = val,
            15 => self.pc = val,
            _ => unimplemented!("TODO banked registers"),
        }
    }

    /// Resets the cpu
    pub fn reset(&mut self) {
        self.pc = 0;
        self.cpsr = 0;
        self.mode = CpuMode::System;
        self.state = CpuState::ARM;
    }

    fn word_size(&self) -> usize {
        match self.state {
            CpuState::ARM => 4,
            CpuState::THUMB => 2,
        }
    }

    fn advance_pc(&mut self) {
        self.pc = self.pc.wrapping_add(self.word_size() as u32)
    }

    fn step_arm(&mut self, sysbus: &mut SysBus) -> CpuResult<()> {
        Err(CpuError::IllegalInstruction)
    }

    pub fn step(&mut self, sysbus: &mut SysBus) -> CpuResult<()> {
        match self.state {
            CpuState::ARM => self.step_arm(sysbus)?,
            CpuState::THUMB => unimplemented!("thumb not implemented :("),
        };

        self.advance_pc();

        Ok(())
    }
}
