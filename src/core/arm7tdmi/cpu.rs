use std::fmt;

use ansi_term::{Colour, Style};
use num_traits::Num;

pub use super::exception::Exception;
use super::{
    alu::*,
    arm::*,
    bus::{Bus, MemoryAccess, MemoryAccessType, MemoryAccessType::*, MemoryAccessWidth::*},
    psr::RegPSR,
    reg_string,
    thumb::ThumbInstruction,
    Addr, CpuMode, CpuResult, CpuState, DecodedInstruction, InstructionDecoder,
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
            0...14 => self.gpr[r],
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

    /// Helper function for "ldr" instruction that handles misaligned addresses
    pub fn ldr_word(&mut self, addr: Addr, bus: &Bus) -> u32 {
        if addr & 0x3 != 0 {
            let rotation = (addr & 0x3) << 3;
            let value = self.load_32(addr & !0x3, bus);
            self.ror(value, rotation, self.cpsr.C(), false, false)
        } else {
            self.load_32(addr, bus)
        }
    }

    /// Helper function for "ldrh" instruction that handles misaligned addresses
    pub fn ldr_half(&mut self, addr: Addr, bus: &Bus) -> u32 {
        if addr & 0x1 != 0 {
            let rotation = (addr & 0x1) << 3;
            let value = self.load_16(addr & !0x1, bus);
            self.ror(value as u32, rotation, self.cpsr.C(), false, false)
        } else {
            self.load_16(addr, bus) as u32
        }
    }

    /// Helper function for "ldrsh" instruction that handles misaligned addresses
    pub fn ldr_sign_half(&mut self, addr: Addr, bus: &Bus) -> u32 {
        if addr & 0x1 != 0 {
            self.load_8(addr, bus) as i8 as i32 as u32
        } else {
            self.load_16(addr, bus) as i16 as i32 as u32
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
        // println!("<cycle I-Cyclel> total: {}", self.cycles);
        self.cycles += 1;
    }

    pub fn add_cycles(&mut self, addr: Addr, bus: &Bus, access: MemoryAccess) {
        // println!("<cycle {:#x} {}> total: {}", addr, access, self.cycles);
        self.cycles += bus.get_cycles(addr, access);
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

    pub fn load_32(&mut self, addr: Addr, bus: &Bus) -> u32 {
        self.add_cycles(addr, bus, self.cycle_type(addr) + MemoryAccess32);
        self.memreq = addr;
        bus.read_32(addr)
    }

    pub fn load_16(&mut self, addr: Addr, bus: &Bus) -> u16 {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess16);
        self.memreq = addr;
        bus.read_16(addr)
    }

    pub fn load_8(&mut self, addr: Addr, bus: &Bus) -> u8 {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess8);
        self.memreq = addr;
        bus.read_8(addr)
    }

    pub fn store_32(&mut self, addr: Addr, value: u32, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess32);
        self.memreq = addr;
        bus.write_32(addr, value);
    }

    pub fn store_16(&mut self, addr: Addr, value: u16, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess16);
        self.memreq = addr;
        bus.write_16(addr, value);
    }

    pub fn store_8(&mut self, addr: Addr, value: u8, bus: &mut Bus) {
        let cycle_type = self.cycle_type(addr);
        self.add_cycles(addr, bus, cycle_type + MemoryAccess8);
        self.memreq = addr;
        bus.write_8(addr, value);
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

    pub fn exec_swi(&mut self) -> CpuExecResult {
        self.exception(Exception::SoftwareInterrupt);
        self.flush_pipeline();
        Ok(())
    }

    fn step_arm_exec(&mut self, insn: u32, sb: &mut Bus) -> CpuResult<()> {
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

    fn arm(&mut self, sb: &mut Bus) -> CpuResult<()> {
        let pc = self.pc;

        // fetch
        let fetched_now = self.load_32(pc, sb);
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

    fn step_thumb_exec(&mut self, insn: u16, sb: &mut Bus) -> CpuResult<()> {
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

    fn thumb(&mut self, sb: &mut Bus) -> CpuResult<()> {
        let pc = self.pc;

        // fetch
        let fetched_now = self.load_16(pc, sb);
        let executed_now = self.decoded_thumb;

        // decode
        self.decoded_thumb = self.fetched_thumb;
        self.fetched_thumb = fetched_now;

        // execute
        self.step_thumb_exec(executed_now, sb)?;
        Ok(())
    }

    pub fn flush_pipeline(&mut self) {
        self.pipeline_state = PipelineState::Refill1;
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    pub fn step(&mut self, bus: &mut Bus) -> CpuResult<()> {
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
    pub fn step_one(&mut self, bus: &mut Bus) -> CpuResult<DecodedInstruction> {
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
