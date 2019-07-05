use std::fmt;

use ansi_term::{Colour, Style};
use num_traits::Num;

pub use super::exception::Exception;
use super::{
    arm::*,
    bus::{Bus, MemoryAccess, MemoryAccessType, MemoryAccessType::*, MemoryAccessWidth::*},
    psr::RegPSR,
    reg_string,
    thumb::ThumbInstruction,
    Addr, CpuMode, CpuResult, CpuState, DecodedInstruction, InstructionDecoder,
};

#[derive(Debug)]
pub struct PipelineContext<D, N>
where
    D: InstructionDecoder,
    N: Num,
{
    fetched: Option<(Addr, N)>,
    decoded: Option<D>,
}

impl<D, N> Default for PipelineContext<D, N>
where
    D: InstructionDecoder,
    N: Num,
{
    fn default() -> PipelineContext<D, N> {
        PipelineContext {
            fetched: None,
            decoded: None,
        }
    }
}

impl<D, N> PipelineContext<D, N>
where
    D: InstructionDecoder,
    N: Num,
{
    fn flush(&mut self) {
        self.fetched = None;
        self.decoded = None;
    }

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

    pub pipeline_arm: PipelineContext<ArmInstruction, u32>,
    pub pipeline_thumb: PipelineContext<ThumbInstruction, u16>,
    cycles: usize,

    // store the gpr before executing an instruction to show diff in the Display impl
    gpr_previous: [u32; 15],

    memreq: Addr,

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
            memreq: 0xffff_0000, // set memreq to an invalid addr so the first load cycle will be non-sequential
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
        self.gpr_banked_r14[curr_index] = self.gpr[14];

        self.gpr[13] = self.gpr_banked_r13[next_index];
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
        let next_index = new_mode.bank_index();
        self.gpr_banked_r14[next_index] = self
            .pc
            .wrapping_sub(self.word_size() as u32)
            .wrapping_add(4);
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

    pub fn cycles(&self) -> usize {
        self.cycles
    }

    pub fn add_cycle(&mut self) {
        println!("<cycle I-Cyclel> total: {}", self.cycles);
        self.cycles += 1;
    }

    pub fn add_cycles(&mut self, addr: Addr, bus: &Bus, access: MemoryAccess) {
        println!("<cycle {:#x} {}> total: {}", addr, access, self.cycles);
        self.cycles += bus.get_cycles(addr, access);
    }

    pub fn cycle_type(&self, addr: Addr) -> MemoryAccessType {
        if addr == self.memreq || addr == self.memreq + (self.word_size() as Addr) {
            Seq
        } else {
            NonSeq
        }
    }

    pub fn get_required_multipiler_array_cycles(&self, rs: i32) -> usize {
        if rs & 0xff == rs {
            1
        } else if rs & 0xffff == rs {
            2
        } else if rs & 0xffffff == rs {
            3
        } else {
            4
        }
    }

    pub fn load_32(&mut self, addr: Addr, bus: &mut Bus) -> u32 {
        self.add_cycles(addr, bus, self.cycle_type(addr) + MemoryAccess32);
        self.memreq = addr;
        bus.read_32(addr)
    }

    pub fn load_16(&mut self, addr: Addr, bus: &mut Bus) -> u16 {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess16);
        self.memreq = addr;
        bus.read_16(addr)
    }

    pub fn load_8(&mut self, addr: Addr, bus: &mut Bus) -> u8 {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess8);
        self.memreq = addr;
        bus.read_8(addr)
    }

    pub fn store_32(&mut self, addr: Addr, value: u32, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess32);
        self.memreq = addr;
        bus.write_32(addr, value).expect("store_32 error");
    }

    pub fn store_16(&mut self, addr: Addr, value: u16, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess16);
        self.memreq = addr;
        bus.write_16(addr, value).expect("store_16 error");
    }

    pub fn store_8(&mut self, addr: Addr, value: u8, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess8);
        self.memreq = addr;
        bus.write_8(addr, value).expect("store_16 error");
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

    fn step_thumb(
        &mut self,
        bus: &mut Bus,
    ) -> CpuResult<(Option<DecodedInstruction>, CpuPipelineAction)> {
        // fetch
        // let new_fetched = bus.read_16(self.pc);
        let new_fetched = self.load_16(self.pc, bus);

        // decode
        let new_decoded = match self.pipeline_thumb.fetched {
            Some((addr, i)) => {
                let insn = ThumbInstruction::decode(i, addr)?;
                Some(insn)
            }
            None => None,
        };

        // exec
        let result = match self.pipeline_thumb.decoded {
            Some(d) => {
                self.gpr_previous = self.get_registers();
                let action = self.exec_thumb(bus, d)?;
                Ok((Some(DecodedInstruction::Thumb(d)), action))
            }
            None => Ok((None, CpuPipelineAction::IncPC)),
        };

        self.pipeline_thumb.fetched = Some((self.pc, new_fetched));
        if let Some(d) = new_decoded {
            self.pipeline_thumb.decoded = Some(d);
        }

        result
    }

    fn step_arm(
        &mut self,
        bus: &mut Bus,
    ) -> CpuResult<(Option<DecodedInstruction>, CpuPipelineAction)> {
        // let new_fetched = bus.read_32(self.pc);
        let new_fetched = self.load_32(self.pc, bus);

        // decode
        let new_decoded = match self.pipeline_arm.fetched {
            Some((addr, i)) => {
                let insn = ArmInstruction::decode(i, addr)?;
                Some(insn)
            }
            None => None,
        };

        // exec
        let result = match self.pipeline_arm.decoded {
            Some(d) => {
                self.gpr_previous = self.get_registers();
                let action = self.exec_arm(bus, d)?;
                Ok((Some(DecodedInstruction::Arm(d)), action))
            }
            None => Ok((None, CpuPipelineAction::IncPC)),
        };

        self.pipeline_arm.fetched = Some((self.pc, new_fetched));
        if let Some(d) = new_decoded {
            self.pipeline_arm.decoded = Some(d);
        }

        result
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    pub fn step(&mut self, bus: &mut Bus) -> CpuResult<Option<DecodedInstruction>> {
        let (executed_instruction, pipeline_action) = match self.cpsr.state() {
            CpuState::ARM => self.step_arm(bus),
            CpuState::THUMB => self.step_thumb(bus),
        }?;

        match pipeline_action {
            CpuPipelineAction::IncPC => self.advance_pc(),
            CpuPipelineAction::Flush => {
                self.pipeline_arm.flush();
                self.pipeline_thumb.flush();
            }
        }

        Ok(executed_instruction)
    }

    /// Get's the address of the next instruction that is going to be executed
    pub fn get_next_pc(&self) -> Addr {
        match self.cpsr.state() {
            CpuState::ARM => {
                if self.pipeline_arm.is_flushed() {
                    self.pc as Addr
                } else if self.pipeline_arm.is_only_fetched() {
                    self.pipeline_arm.fetched.unwrap().0
                } else if self.pipeline_arm.is_ready_to_execute() {
                    self.pipeline_arm.decoded.unwrap().pc
                } else {
                    unreachable!()
                }
            }
            CpuState::THUMB => {
                if self.pipeline_thumb.is_flushed() {
                    self.pc as Addr
                } else if self.pipeline_thumb.is_only_fetched() {
                    self.pipeline_thumb.fetched.unwrap().0
                } else if self.pipeline_thumb.is_ready_to_execute() {
                    self.pipeline_thumb.decoded.unwrap().pc
                } else {
                    unreachable!()
                }
            }
        }
    }

    /// A step that returns only once an instruction was executed.
    /// Returns the address of PC before executing an instruction,
    /// and the address of the next instruction to be executed;
    pub fn step_debugger(&mut self, bus: &mut Bus) -> CpuResult<DecodedInstruction> {
        loop {
            if let Some(i) = self.step(bus)? {
                return Ok(i);
            }
        }
    }
}

impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "ARM7TDMI Core Status:")?;
        writeln!(f, "\tCycles: {}", self.cycles)?;
        writeln!(f, "\tCPSR: {}", self.cpsr)?;
        writeln!(f, "\tGeneral Purpose Registers:")?;
        let reg_normal_style = Style::new().bold();
        let reg_dirty_style = Colour::Black.bold().on(Colour::Yellow);
        let gpr = self.get_registers();
        for i in 0..15 {
            let mut reg_name = reg_string(i).to_string();
            reg_name.make_ascii_uppercase();

            let style = if gpr[i] != self.gpr_previous[i] {
                &reg_dirty_style
            } else {
                &reg_normal_style
            };

            let entry = format!("\t{:-3} = 0x{:08x}", reg_name, gpr[i]);

            write!(
                f,
                "{}{}",
                style.paint(entry),
                if (i + 1) % 4 == 0 { "\n" } else { "" }
            )?;
        }
        let pc = format!("\tPC  = 0x{:08x}", self.get_next_pc());
        writeln!(f, "{}", reg_normal_style.paint(pc))
    }
}
