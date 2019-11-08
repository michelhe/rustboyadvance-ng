use std::fmt;

use ansi_term::{Colour, Style};

pub use super::exception::Exception;
use super::{
    arm::*, bus::Bus, psr::RegPSR, reg_string, thumb::ThumbInstruction, Addr, CpuMode, CpuResult,
    CpuState, DecodedInstruction, InstructionDecoder,
};

use super::super::sysbus::{
    MemoryAccess, MemoryAccessType, MemoryAccessType::*, MemoryAccessWidth::*, SysBus,
};

#[derive(Debug, PartialEq)]
enum PipelineState {
    Refill1,
    Refill2,
    Execute,
}

impl Default for PipelineState {
    fn default() -> PipelineState {
        PipelineState::Refill1
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

    pub bs_carry_out: bool,

    pipeline_state: PipelineState,
    fetched_arm: u32,
    decoded_arm: u32,
    fetched_thumb: u16,
    decoded_thumb: u16,
    last_executed: Option<DecodedInstruction>,

    pub cycles: usize,

    // store the gpr before executing an instruction to show diff in the Display impl
    gpr_previous: [u32; 15],

    memreq: Addr,
    pub breakpoints: Vec<u32>,

    pub verbose: bool,
}

pub type CpuExecResult = CpuResult<()>;

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

    pub fn get_reg(&self, r: usize) -> u32 {
        match r {
            0..=14 => self.gpr[r],
            15 => self.pc,
            _ => panic!("invalid register {}", r),
        }
    }

    pub fn get_reg_user(&mut self, r: usize) -> u32 {
        match r {
            0..=7 => self.gpr[r],
            8..=12 => {
                if self.cpsr.mode() == CpuMode::Fiq {
                    self.gpr[r]
                } else {
                    self.gpr_banked_old_r8_12[r - 8]
                }
            }
            13 => self.gpr_banked_r13[0],
            14 => self.gpr_banked_r14[0],
            _ => panic!("invalid register"),
        }
    }

    pub fn set_reg(&mut self, r: usize, val: u32) {
        match r {
            0...14 => self.gpr[r] = val,
            15 => self.pc = val & !1,
            _ => panic!("invalid register"),
        }
    }

    pub fn set_reg_user(&mut self, r: usize, val: u32) {
        match r {
            0..=7 => self.gpr[r] = val,
            8..=12 => {
                if self.cpsr.mode() == CpuMode::Fiq {
                    self.gpr[r] = val;
                } else {
                    self.gpr_banked_old_r8_12[r - 8] = val;
                }
            }
            13 => {
                self.gpr_banked_r13[0] = val;
            }
            14 => {
                self.gpr_banked_r14[0] = val;
            }
            _ => panic!("invalid register"),
        }
    }

    pub fn write_32(&mut self, addr: Addr, value: u32, bus: &mut SysBus) {
        bus.write_32(addr & !0x3, value);
    }

    pub fn write_16(&mut self, addr: Addr, value: u16, bus: &mut SysBus) {
        bus.write_16(addr & !0x1, value);
    }

    pub fn write_8(&mut self, addr: Addr, value: u8, bus: &mut SysBus) {
        bus.write_8(addr, value);
    }

    /// Helper function for "ldr" instruction that handles misaligned addresses
    pub fn ldr_word(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x3 != 0 {
            let rotation = (addr & 0x3) << 3;
            let value = bus.read_32(addr & !0x3);
            self.ror(value, rotation, self.cpsr.C(), false, false)
        } else {
            bus.read_32(addr)
        }
    }

    /// Helper function for "ldrh" instruction that handles misaligned addresses
    pub fn ldr_half(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x1 != 0 {
            let rotation = (addr & 0x1) << 3;
            let value = bus.read_16(addr & !0x1);
            self.ror(value as u32, rotation, self.cpsr.C(), false, false)
        } else {
            bus.read_16(addr) as u32
        }
    }

    /// Helper function for "ldrsh" instruction that handles misaligned addresses
    pub fn ldr_sign_half(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x1 != 0 {
            bus.read_8(addr) as i8 as i32 as u32
        } else {
            bus.read_16(addr) as i16 as i32 as u32
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
        // let next_index = new_mode.bank_index();
        // self.gpr_banked_r14[next_index] = self.get_next_pc();
    }

    /// Resets the cpu
    pub fn reset(&mut self, sb: &mut SysBus) {
        self.exception(sb, Exception::Reset);
    }

    pub fn word_size(&self) -> usize {
        match self.cpsr.state() {
            CpuState::ARM => 4,
            CpuState::THUMB => 2,
        }
    }

    pub fn cycles(&self) -> usize {
        self.cycles
    }

    pub fn add_cycle(&mut self) {
        // println!("<cycle I-Cyclel> total: {}", self.cycles);
        self.cycles += 1;
    }

    pub fn add_cycles(&mut self, addr: Addr, bus: &SysBus, access: MemoryAccess) {
        let cycles_to_add = 1 + bus.get_cycles(addr, access);
        // println!("<cycle {:#x} {}> took: {}", addr, access, cycles_to_add);
        self.cycles += cycles_to_add;
    }

    pub fn cycle_type(&self, addr: Addr) -> MemoryAccessType {
        if addr == self.memreq || addr == self.memreq.wrapping_add(self.word_size() as Addr) {
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

    #[allow(non_snake_case)]
    pub fn S_cycle32(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, Seq + MemoryAccess32);
    }

    #[allow(non_snake_case)]
    pub fn S_cycle16(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, Seq + MemoryAccess16);
    }

    #[allow(non_snake_case)]
    pub fn S_cycle8(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, Seq + MemoryAccess8);
    }

    #[allow(non_snake_case)]
    pub fn N_cycle32(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, NonSeq + MemoryAccess32);
    }

    #[allow(non_snake_case)]
    pub fn N_cycle16(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, NonSeq + MemoryAccess16);
    }

    #[allow(non_snake_case)]
    pub fn N_cycle8(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += 1;
        self.cycles += sb.get_cycles(addr, NonSeq + MemoryAccess8);
    }

    pub fn check_arm_cond(&self, cond: ArmCond) -> bool {
        use ArmCond::*;
        match cond {
            EQ => self.cpsr.Z(),
            NE => !self.cpsr.Z(),
            HS => self.cpsr.C(),
            LO => !self.cpsr.C(),
            MI => self.cpsr.N(),
            PL => !self.cpsr.N(),
            VS => self.cpsr.V(),
            VC => !self.cpsr.V(),
            HI => self.cpsr.C() && !self.cpsr.Z(),
            LS => !self.cpsr.C() || self.cpsr.Z(),
            GE => self.cpsr.N() == self.cpsr.V(),
            LT => self.cpsr.N() != self.cpsr.V(),
            GT => !self.cpsr.Z() && (self.cpsr.N() == self.cpsr.V()),
            LE => self.cpsr.Z() || (self.cpsr.N() != self.cpsr.V()),
            AL => true,
        }
    }

    pub fn exec_swi(&mut self, sb: &mut SysBus) -> CpuExecResult {
        match self.cpsr.state() {
            CpuState::ARM => self.N_cycle32(sb, self.pc),
            CpuState::THUMB => self.N_cycle16(sb, self.pc),
        };
        self.exception(sb, Exception::SoftwareInterrupt);
        Ok(())
    }

    fn step_arm_exec(&mut self, insn: u32, sb: &mut SysBus) -> CpuResult<()> {
        let pc = self.pc;
        match self.pipeline_state {
            PipelineState::Refill1 => {
                self.pc = pc.wrapping_add(4);
                self.pipeline_state = PipelineState::Refill2;
            }
            PipelineState::Refill2 => {
                self.pc = pc.wrapping_add(4);
                self.pipeline_state = PipelineState::Execute;
            }
            PipelineState::Execute => {
                let insn = ArmInstruction::decode(insn, self.pc.wrapping_sub(8))?;
                self.gpr_previous = self.get_registers();
                self.exec_arm(sb, insn)?;
                if !self.did_pipeline_flush() {
                    self.pc = pc.wrapping_add(4);
                }
                self.last_executed = Some(DecodedInstruction::Arm(insn));
            }
        }
        Ok(())
    }

    fn arm(&mut self, sb: &mut SysBus) -> CpuResult<()> {
        let pc = self.pc;

        // fetch
        let fetched_now = sb.read_32(pc);
        let executed_now = self.decoded_arm;

        // decode
        self.decoded_arm = self.fetched_arm;
        self.fetched_arm = fetched_now;

        // execute
        self.step_arm_exec(executed_now, sb)?;
        Ok(())
    }

    pub fn did_pipeline_flush(&self) -> bool {
        self.pipeline_state == PipelineState::Refill1
    }

    fn step_thumb_exec(&mut self, insn: u16, sb: &mut SysBus) -> CpuResult<()> {
        let pc = self.pc;
        match self.pipeline_state {
            PipelineState::Refill1 => {
                self.pc = pc.wrapping_add(2);
                self.pipeline_state = PipelineState::Refill2;
            }
            PipelineState::Refill2 => {
                self.pc = pc.wrapping_add(2);
                self.pipeline_state = PipelineState::Execute;
            }
            PipelineState::Execute => {
                let insn = ThumbInstruction::decode(insn, self.pc.wrapping_sub(4))?;
                self.gpr_previous = self.get_registers();
                self.exec_thumb(sb, insn)?;
                if !self.did_pipeline_flush() {
                    self.pc = pc.wrapping_add(2);
                }
                self.last_executed = Some(DecodedInstruction::Thumb(insn));
            }
        }
        Ok(())
    }

    fn thumb(&mut self, sb: &mut SysBus) -> CpuResult<()> {
        let pc = self.pc;

        // fetch
        let fetched_now = sb.read_16(pc);
        let executed_now = self.decoded_thumb;

        // decode
        self.decoded_thumb = self.fetched_thumb;
        self.fetched_thumb = fetched_now;

        // execute
        self.step_thumb_exec(executed_now, sb)?;
        Ok(())
    }

    pub fn flush_pipeline(&mut self, sb: &mut SysBus) {
        self.pipeline_state = PipelineState::Refill1;
        match self.cpsr.state() {
            CpuState::ARM => {
                self.N_cycle32(sb, self.pc);
                self.S_cycle32(sb, self.pc + 4);
            }
            CpuState::THUMB => {
                self.N_cycle16(sb, self.pc);
                self.S_cycle16(sb, self.pc + 2);
            }
        }
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    pub fn step(&mut self, bus: &mut SysBus) -> CpuResult<()> {
        match self.cpsr.state() {
            CpuState::ARM => self.arm(bus),
            CpuState::THUMB => self.thumb(bus),
        }
    }

    /// Get's the address of the next instruction that is going to be executed
    pub fn get_next_pc(&self) -> Addr {
        let insn_size = self.word_size() as u32;
        match self.pipeline_state {
            PipelineState::Refill1 => self.pc,
            PipelineState::Refill2 => self.pc - insn_size,
            PipelineState::Execute => self.pc - 2 * insn_size,
        }
    }

    /// A step that returns only once an instruction was executed.
    /// Returns the address of PC before executing an instruction,
    /// and the address of the next instruction to be executed;
    pub fn step_one(&mut self, bus: &mut SysBus) -> CpuResult<DecodedInstruction> {
        loop {
            match self.pipeline_state {
                PipelineState::Execute => {
                    self.step(bus)?;
                    return Ok(self.last_executed.unwrap());
                }
                _ => {
                    self.step(bus)?;
                }
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
