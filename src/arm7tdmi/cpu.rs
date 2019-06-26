use std::convert::TryFrom;
use std::fmt;

use colored::*;

use super::reg_string;
use super::arm::*;
use super::psr::{CpuMode, CpuState, RegPSR};
use super::sysbus::SysBus;

#[derive(Debug, PartialEq)]
pub enum CpuInstruction {
    Arm(ArmInstruction),
    Thumb,
}

#[derive(Debug, PartialEq)]
pub enum CpuError {
    ArmDecodeError(ArmDecodeError),
    IllegalInstruction(CpuInstruction),
    UnimplementedCpuInstruction(CpuInstruction),
}

impl From<ArmDecodeError> for CpuError {
    fn from(e: ArmDecodeError) -> CpuError {
        CpuError::ArmDecodeError(e)
    }
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CpuError::ArmDecodeError(e) => write!(
                f,
                "arm decoding error at address @0x{:08x} (instruction 0x{:08x}): {:?}",
                e.addr, e.insn, e.kind
            ),
            CpuError::UnimplementedCpuInstruction(CpuInstruction::Arm(insn)) => write!(
                f,
                "unimplemented instruction: 0x{:08x}:\t0x{:08x}\t{}",
                insn.pc, insn.raw, insn
            ),
            CpuError::IllegalInstruction(CpuInstruction::Arm(insn)) => write!(
                f,
                "illegal instruction at address @0x{:08x} (0x{:08x})",
                insn.pc, insn.raw
            ),
            e => write!(f, "error: {:#x?}", e),
        }
    }
}

pub type CpuResult<T> = Result<T, CpuError>;

pub struct CpuModeContext {
    // r8-r14
    banked_gpr: [u32; 7],
    spsr: u32,
}

#[derive(Debug)]
pub struct Core {
    pub pc: u32,
    // r0-r7
    gpr: [u32; 15],
    pub cpsr: RegPSR,
    pub verbose: bool,
}

#[derive(Debug, PartialEq)]
pub enum CpuPipelineAction {
    AdvancePc,
    Branch,
}

pub type CpuExecResult = CpuResult<(CpuInstruction, CpuPipelineAction)>;

impl Core {
    pub fn new() -> Core {
        Core {
            pc: 0,
            gpr: [0; 15],
            cpsr: RegPSR::new(),
            verbose: false,
        }
    }

    pub fn set_verbose(&mut self, v: bool) {
        self.verbose = v;
    }

    pub fn get_reg(&self, reg_num: usize) -> u32 {
        match reg_num {
            0...14 => self.gpr[reg_num],
            15 => self.pc,
            _ => panic!("invalid register")
            // _ => 0x12345678 // unimplemented!("TODO banked registers"),
        }
    }

    pub fn set_reg(&mut self, reg_num: usize, val: u32) {
        match reg_num {
            0...14 => self.gpr[reg_num] = val,
            15 => self.pc = val,
            _ => panic!("invalid register")
            // _ => unimplemented!("TODO banked registers"),
        }
    }

    /// Resets the cpu
    pub fn reset(&mut self) {
        self.pc = 0;
        self.cpsr.set(0);
    }

    fn word_size(&self) -> usize {
        match self.cpsr.state() {
            CpuState::ARM => 4,
            CpuState::THUMB => 2,
        }
    }

    fn advance_pc(&mut self) {
        self.pc = self.pc.wrapping_add(self.word_size() as u32)
    }

    fn step_arm(&mut self, sysbus: &mut SysBus) -> CpuExecResult {
        // fetch
        let insn = sysbus.read_32(self.pc);
        // decode
        let insn = ArmInstruction::try_from((insn, self.pc))?;
        // exec
        self.exec_arm(sysbus, insn)
    }

    pub fn step(&mut self, sysbus: &mut SysBus) -> CpuResult<()> {
        let (executed_insn, pipeline_action) = match self.cpsr.state() {
            CpuState::ARM => self.step_arm(sysbus),
            CpuState::THUMB => unimplemented!("thumb not implemented :("),
        }?;

        if self.verbose {
            if let CpuInstruction::Arm(insn) = executed_insn {
                println!("{:8x}:\t{:08x} \t{}", insn.pc, insn.raw, insn)
            }
        }

        if CpuPipelineAction::AdvancePc == pipeline_action {
            self.advance_pc();
        }

        Ok(())
    }
}

impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "ARM7TDMI Core")?;
        writeln!(f, "REGISTERS:")?;
        for i in 0..16 {
            let mut reg = reg_string(i).to_string();
            reg.make_ascii_uppercase();
            writeln!(f, "\t{}\t= 0x{:08x}", reg.bright_yellow(), self.get_reg(i))?;
        }
        write!(f, "CPSR: {}", self.cpsr)
    }
}
