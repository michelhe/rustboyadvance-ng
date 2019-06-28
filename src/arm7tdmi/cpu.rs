use std::convert::TryFrom;
use std::fmt;

use crate::num_traits::FromPrimitive;
use colored::*;

use super::arm::*;
pub use super::exception::Exception;
use super::psr::RegPSR;
use super::reg_string;
use super::sysbus::SysBus;

#[derive(Debug, PartialEq)]
pub enum CpuInstruction {
    Arm(ArmInstruction),
    Thumb,
}

#[derive(Debug, PartialEq, Primitive, Copy, Clone)]
#[repr(u8)]
pub enum CpuState {
    ARM = 0,
    THUMB = 1,
}

impl fmt::Display for CpuState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use CpuState::*;
        match self {
            ARM => write!(f, "ARM"),
            THUMB => write!(f, "THUMB"),
        }
    }
}

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
pub enum CpuMode {
    User = 0b10000,
    Fiq = 0b10001,
    Irq = 0b10010,
    Supervisor = 0b10011,
    Abort = 0b10111,
    Undefined = 0b11011,
    System = 0b11111,
}

impl CpuMode {
    pub fn spsr_index(&self) -> Option<usize> {
        match self {
            CpuMode::Fiq => Some(0),
            CpuMode::Irq => Some(1),
            CpuMode::Supervisor => Some(2),
            CpuMode::Abort => Some(3),
            CpuMode::Undefined => Some(4),
            _ => None,
        }
    }

    pub fn bank_index(&self) -> usize {
        match self {
            CpuMode::User | CpuMode::System => 0,
            CpuMode::Fiq => 1,
            CpuMode::Irq => 2,
            CpuMode::Supervisor => 3,
            CpuMode::Abort => 4,
            CpuMode::Undefined => 5,
        }
    }
}

impl fmt::Display for CpuMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use CpuMode::*;
        match self {
            User => write!(f, "USR"),
            Fiq => write!(f, "FIQ"),
            Irq => write!(f, "IRQ"),
            Supervisor => write!(f, "SVC"),
            Abort => write!(f, "ABT"),
            Undefined => write!(f, "UND"),
            System => write!(f, "SYS"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CpuError {
    ArmDecodeError(ArmDecodeError),
    IllegalInstruction,
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
            CpuError::IllegalInstruction => write!(
                f,
                "illegal instruction"
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

#[derive(Debug, Default)]
pub struct Core {
    pub pc: u32,
    // r0-r7
    pub gpr: [u32; 15],
    // r13 and r14 are banked for all modes. System&User mode share them
    pub gpr_banked_r13: [u32; 6],
    pub gpr_banked_r14: [u32; 6],
    // r8-r12 are banked for fiq mode
    pub gpr_banked_old_r8_12: [u32; 5],
    pub gpr_banked_fiq_r8_12: [u32; 5],

    pub cpsr: RegPSR,
    pub spsr: [RegPSR; 5],

    pub verbose: bool,
}

#[derive(Debug, PartialEq)]
pub enum CpuPipelineAction {
    AdvanceProgramCounter,
    Branch,
}

pub type CpuExecResult = CpuResult<(CpuInstruction, CpuPipelineAction)>;

impl Core {
    pub fn new() -> Core {
        Core {
            ..Default::default()
        }
    }

    pub fn set_verbose(&mut self, v: bool) {
        self.verbose = v;
    }

    pub fn get_reg(&self, reg_num: usize) -> u32 {
        match reg_num {
            0...14 => self.gpr[reg_num],
            15 => self.pc,
            _ => panic!("invalid register"),
        }
    }

    pub fn set_reg(&mut self, reg_num: usize, val: u32) {
        match reg_num {
            0...14 => self.gpr[reg_num] = val,
            15 => self.pc = val,
            _ => panic!("invalid register"),
        }
    }

    fn map_banked_registers(&mut self, curr_mode: CpuMode, new_mode: CpuMode) {
        let next_index = new_mode.bank_index();
        let curr_index = curr_mode.bank_index();

        self.gpr_banked_r13[curr_index] = self.gpr[13];
        self.gpr[13] = self.gpr_banked_r13[next_index];

        self.gpr_banked_r14[curr_index] = self.gpr[14];
        self.gpr_banked_r14[next_index] = self.pc; // Store the return address in LR_mode
        self.gpr[14] = self.gpr_banked_r14[next_index];

        if new_mode == CpuMode::Fiq {
            for r in 0..5 {
                self.gpr_banked_old_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = self.gpr_banked_fiq_r8_12[r];
            }
        } else if curr_mode == CpuMode::Fiq {
            for r in 0..5 {
                self.gpr_banked_fiq_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = self.gpr_banked_old_r8_12[r];
            }
        }
    }

    pub fn change_mode(&mut self, new_mode: CpuMode) {
        let curr_mode = self.cpsr.mode();
        // Copy CPSR to SPSR_mode
        if let Some(index) = new_mode.spsr_index() {
            self.spsr[index] = self.cpsr;
        }
        self.map_banked_registers(curr_mode, new_mode);
    }

    /// Resets the cpu
    pub fn reset(&mut self) {
        self.exception(Exception::Reset);
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

        if CpuPipelineAction::AdvanceProgramCounter == pipeline_action {
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
