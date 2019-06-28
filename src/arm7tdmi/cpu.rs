use std::convert::TryFrom;
use std::fmt;

use ansi_term::{Colour, Style};

use super::*;

pub use super::exception::Exception;
use super::psr::RegPSR;
use super::reg_string;
use super::sysbus::SysBus;

type Addr = u32;

#[derive(Debug, Default)]
pub struct PipelineContext {
    fetched: Option<(Addr, u32)>,
    decoded: Option<ArmInstruction>,
}

impl PipelineContext {
    fn is_flushed(&self) -> bool {
        self.fetched.is_none() && self.decoded.is_none()
    }

    fn is_only_fetched(&self) -> bool {
        self.fetched.is_some() && self.decoded.is_none()
    }

    fn is_ready_to_execute(&self) -> bool {
        self.fetched.is_some() && self.decoded.is_some()
    }
}

#[derive(Debug, Default)]
pub struct Core {
    pub pc: u32,
    pub gpr: [u32; 15],
    // r13 and r14 are banked for all modes. System&User mode share them
    pub gpr_banked_r13: [u32; 6],
    pub gpr_banked_r14: [u32; 6],
    // r8-r12 are banked for fiq mode
    pub gpr_banked_old_r8_12: [u32; 5],
    pub gpr_banked_fiq_r8_12: [u32; 5],

    pub cpsr: RegPSR,
    pub spsr: [RegPSR; 5],

    pub pipeline: PipelineContext,
    cycles: usize,

    // store the gpr before executing an instruction to show diff in the Display impl
    gpr_previous: [u32; 15],

    pub verbose: bool,
}

#[derive(Debug, PartialEq)]
pub enum CpuPipelineAction {
    IncPC,
    Flush,
}

pub type CpuExecResult = CpuResult<CpuPipelineAction>;

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

    pub fn get_registers(&self) -> [u32; 15] {
        self.gpr.clone()
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

    pub fn word_size(&self) -> usize {
        match self.cpsr.state() {
            CpuState::ARM => 4,
            CpuState::THUMB => 2,
        }
    }

    fn advance_pc(&mut self) {
        self.pc = self.pc.wrapping_add(self.word_size() as u32)
    }

    pub fn check_arm_cond(&self, cond: ArmCond) -> bool {
        use ArmCond::*;
        match cond {
            Equal => self.cpsr.Z(),
            NotEqual => !self.cpsr.Z(),
            UnsignedHigherOrSame => self.cpsr.C(),
            UnsignedLower => !self.cpsr.C(),
            Negative => self.cpsr.N(),
            PositiveOrZero => !self.cpsr.N(),
            Overflow => self.cpsr.V(),
            NoOverflow => !self.cpsr.V(),
            UnsignedHigher => self.cpsr.C() && !self.cpsr.Z(),
            UnsignedLowerOrSame => !self.cpsr.C() && self.cpsr.Z(),
            GreaterOrEqual => self.cpsr.N() == self.cpsr.V(),
            LessThan => self.cpsr.N() != self.cpsr.V(),
            GreaterThan => !self.cpsr.Z() && (self.cpsr.N() == self.cpsr.V()),
            LessThanOrEqual => self.cpsr.Z() || (self.cpsr.N() != self.cpsr.V()),
            Always => true,
        }
    }

    fn step_arm(
        &mut self,
        sysbus: &mut SysBus,
    ) -> CpuResult<(Option<ArmInstruction>, CpuPipelineAction)> {
        // fetch
        let new_fetched = sysbus.read_32(self.pc);

        // decode
        let new_decoded = match self.pipeline.fetched {
            Some((addr, i)) => Some(ArmInstruction::try_from((i, addr)).unwrap()),
            None => None,
        };
        // exec
        let result = match self.pipeline.decoded {
            Some(d) => {
                self.gpr_previous = self.get_registers();
                let action = self.exec_arm(sysbus, d)?;
                Ok((Some(d), action))
            }
            None => Ok((None, CpuPipelineAction::IncPC)),
        };

        self.pipeline.fetched = Some((self.pc, new_fetched));
        if let Some(d) = new_decoded {
            self.pipeline.decoded = Some(d);
        }

        result
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    pub fn step(&mut self, sysbus: &mut SysBus) -> CpuResult<Option<ArmInstruction>> {
        if self.cycles > 0 {
            self.cycles -= 1;
            return Ok(None);
        }
        let (executed_instruction, pipeline_action) = match self.cpsr.state() {
            CpuState::ARM => self.step_arm(sysbus),
            CpuState::THUMB => unimplemented!("thumb not implemented :("),
        }?;

        match pipeline_action {
            CpuPipelineAction::IncPC => self.advance_pc(),
            CpuPipelineAction::Flush => {
                self.pipeline.fetched = None;
                self.pipeline.decoded = None;
            }
        }

        Ok(executed_instruction)
    }

    /// Get's the address of the next instruction that is going to be executed
    pub fn get_next_pc(&self) -> Addr {
        if self.pipeline.is_flushed() {
            self.pc
        } else if self.pipeline.is_only_fetched() {
            self.pipeline.fetched.unwrap().0
        } else if self.pipeline.is_ready_to_execute() {
            self.pipeline.decoded.unwrap().pc
        } else {
            unreachable!()
        }
    }

    /// A step that returns only once an instruction was executed.
    /// Returns the address of PC before executing an instruction,
    /// and the address of the next instruction to be executed;
    pub fn step_debugger(&mut self, sysbus: &mut SysBus) -> CpuResult<ArmInstruction> {
        loop {
            if let Some(i) = self.step(sysbus)? {
                return Ok(i);
            }
        }
    }
}

impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "ARM7TDMI Core Status:")?;
        writeln!(f, "\tCPSR: {}", self.cpsr)?;
        writeln!(f, "\tGeneral Purpose Registers:")?;
        let reg_normal_style = Style::new().bold();
        let reg_dirty_style = Colour::Green.bold().on(Colour::Yellow);
        let gpr = self.get_registers();
        for i in 0..15 {
            let mut reg_name = reg_string(i).to_string();
            reg_name.make_ascii_uppercase();

            let style = if gpr[i] != self.gpr_previous[i] {
                &reg_dirty_style
            } else {
                &reg_normal_style
            };

            let entry = format!("\t{}\t= 0x{:08x}", reg_name, gpr[i]);

            write!(
                f,
                "{}{}",
                style.paint(entry),
                if (i + 1) % 4 == 0 { "\n" } else { "" }
            )?;
        }
        let pc = format!("\tPC\t= 0x{:08x}", self.pc);
        writeln!(f, "{}", reg_normal_style.paint(pc))
    }
}
