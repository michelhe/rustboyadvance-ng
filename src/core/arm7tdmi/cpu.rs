#[cfg(feature = "debugger")]
use super::reg_string;
#[cfg(feature = "debugger")]
use ansi_term::{Colour, Style};
use serde::{Deserialize, Serialize};
#[cfg(feature = "debugger")]
use std::fmt;

use super::arm::ArmCond;
#[cfg(feature = "arm7tdmi_dispatch_table")]
use super::arm::{arm_insn_hash, ARM_LUT};
pub use super::exception::Exception;
#[cfg(feature = "arm7tdmi_dispatch_table")]
use super::thumb::THUMB_LUT;
use super::CpuAction;
#[cfg(feature = "debugger")]
use super::DecodedInstruction;
use super::{arm::*, psr::RegPSR, thumb::ThumbInstruction, Addr, CpuMode, CpuState};

#[cfg(not(feature = "arm7tdmi_dispatch_table"))]
use super::InstructionDecoder;

use crate::core::bus::Bus;
use crate::core::sysbus::{MemoryAccessType::*, MemoryAccessWidth::*, SysBus};

use bit::BitIndex;
use num::FromPrimitive;

#[cfg(feature = "arm7tdmi_dispatch_table")]
use lazy_static;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Core {
    pub pc: u32,
    pub gpr: [u32; 15],
    // r13 and r14 are banked for all modes. System&User mode share them
    pub(super) gpr_banked_r13: [u32; 6],
    pub(super) gpr_banked_r14: [u32; 6],
    // r8-r12 are banked for fiq mode
    pub(super) gpr_banked_old_r8_12: [u32; 5],
    pub(super) gpr_banked_fiq_r8_12: [u32; 5],

    pub cpsr: RegPSR,
    pub(super) spsr: RegPSR,
    pub(super) spsr_bank: [RegPSR; 6],

    pub(super) bs_carry_out: bool,

    pipeline: [u32; 2],

    #[cfg(feature = "debugger")]
    pub last_executed: Option<DecodedInstruction>,

    pub cycles: usize,

    // store the gpr before executing an instruction to show diff in the Display impl
    gpr_previous: [u32; 15],

    memreq: Addr,
    pub breakpoints: Vec<u32>,

    pub verbose: bool,

    pub trace_opcodes: bool,

    pub trace_exceptions: bool,
}

impl Core {
    pub fn new() -> Core {
        #[cfg(feature = "arm7tdmi_dispatch_table")]
        {
            lazy_static::initialize(&ARM_LUT);
            lazy_static::initialize(&ARM_FN_LUT);
            lazy_static::initialize(&THUMB_LUT);
        }

        let cpsr = RegPSR::new(0x0000_00D3);
        Core {
            memreq: 0xffff_0000, // set memreq to an invalid addr so the first load cycle will be non-sequential
            cpsr: cpsr,
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
            0..=14 => self.gpr[r] = val,
            15 => {
                self.pc = {
                    match self.cpsr.state() {
                        CpuState::THUMB => val & !1,
                        CpuState::ARM => val & !3,
                    }
                }
            }
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

    pub(super) fn write_32(&mut self, addr: Addr, value: u32, bus: &mut SysBus) {
        bus.write_32(addr & !0x3, value);
    }

    pub(super) fn write_16(&mut self, addr: Addr, value: u16, bus: &mut SysBus) {
        bus.write_16(addr & !0x1, value);
    }

    pub(super) fn write_8(&mut self, addr: Addr, value: u8, bus: &mut SysBus) {
        bus.write_8(addr, value);
    }

    /// Helper function for "ldr" instruction that handles misaligned addresses
    pub(super) fn ldr_word(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x3 != 0 {
            let rotation = (addr & 0x3) << 3;
            let value = bus.read_32(addr & !0x3);
            self.ror(value, rotation, self.cpsr.C(), false, false)
        } else {
            bus.read_32(addr)
        }
    }

    /// Helper function for "ldrh" instruction that handles misaligned addresses
    pub(super) fn ldr_half(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x1 != 0 {
            let rotation = (addr & 0x1) << 3;
            let value = bus.read_16(addr & !0x1);
            self.ror(value as u32, rotation, self.cpsr.C(), false, false)
        } else {
            bus.read_16(addr) as u32
        }
    }

    /// Helper function for "ldrsh" instruction that handles misaligned addresses
    pub(super) fn ldr_sign_half(&mut self, addr: Addr, bus: &SysBus) -> u32 {
        if addr & 0x1 != 0 {
            bus.read_8(addr) as i8 as i32 as u32
        } else {
            bus.read_16(addr) as i16 as i32 as u32
        }
    }

    pub fn get_registers(&self) -> [u32; 15] {
        self.gpr.clone()
    }

    pub(super) fn change_mode(&mut self, old_mode: CpuMode, new_mode: CpuMode) {
        let new_index = new_mode.bank_index();
        let old_index = old_mode.bank_index();

        if new_index == old_index {
            return;
        }

        self.spsr_bank[old_index] = self.spsr;
        self.gpr_banked_r13[old_index] = self.gpr[13];
        self.gpr_banked_r14[old_index] = self.gpr[14];

        self.spsr = self.spsr_bank[new_index];
        self.gpr[13] = self.gpr_banked_r13[new_index];
        self.gpr[14] = self.gpr_banked_r14[new_index];

        if new_mode == CpuMode::Fiq {
            for r in 0..5 {
                self.gpr_banked_old_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = self.gpr_banked_fiq_r8_12[r];
            }
        } else if old_mode == CpuMode::Fiq {
            for r in 0..5 {
                self.gpr_banked_fiq_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = self.gpr_banked_old_r8_12[r];
            }
        }
        self.cpsr.set_mode(new_mode);
    }

    /// Resets the cpu
    pub fn reset(&mut self, sb: &mut SysBus) {
        self.exception(sb, Exception::Reset, 0);
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

    pub(super) fn add_cycle(&mut self) {
        // println!("<cycle I-Cyclel> total: {}", self.cycles);
        self.cycles += 1;
    }

    pub(super) fn get_required_multipiler_array_cycles(&self, rs: u32) -> usize {
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
    #[inline(always)]
    pub(super) fn S_cycle32(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, Seq, MemoryAccess32);
    }

    #[allow(non_snake_case)]
    #[inline(always)]
    pub(super) fn S_cycle16(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, Seq, MemoryAccess16);
    }

    #[allow(non_snake_case)]
    #[inline(always)]
    pub(super) fn S_cycle8(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, Seq, MemoryAccess8);
    }

    #[allow(non_snake_case)]
    #[inline(always)]
    pub(super) fn N_cycle32(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, NonSeq, MemoryAccess32);
    }

    #[allow(non_snake_case)]
    #[inline(always)]
    pub(super) fn N_cycle16(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, NonSeq, MemoryAccess16);
    }

    #[allow(non_snake_case)]
    #[inline(always)]
    pub(super) fn N_cycle8(&mut self, sb: &SysBus, addr: u32) {
        self.cycles += sb.get_cycles(addr, NonSeq, MemoryAccess8);
    }

    #[inline]
    pub(super) fn check_arm_cond(&self, cond: ArmCond) -> bool {
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

    #[cfg(feature = "debugger")]
    fn debugger_record_step(&mut self, d: DecodedInstruction) {
        self.gpr_previous = self.get_registers();
        self.last_executed = Some(d);
    }

    #[cfg(feature = "arm7tdmi_dispatch_table")]
    fn step_arm_exec(&mut self, insn: u32, sb: &mut SysBus) -> CpuAction {
        let l1_index = ARM_LUT[arm_insn_hash(insn)] as usize;
        let handler_fn = ARM_FN_LUT[l1_index];

        // This is safe because the table can't hold invalid indices
        let arm_format: ArmFormat = unsafe { std::mem::transmute(l1_index as u8) };
        let arm_insn = ArmInstruction::new(insn, self.pc.wrapping_sub(8), arm_format);

        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Arm(arm_insn.clone()));

        (handler_fn)(self, sb, &arm_insn)
    }

    #[cfg(feature = "arm7tdmi_dispatch_table")]
    fn step_thumb_exec(&mut self, insn: u16, sb: &mut SysBus) -> CpuAction {
        let thumb_info = &THUMB_LUT[(insn >> 6) as usize];
        let thumb_insn = ThumbInstruction::new(insn, self.pc.wrapping_sub(4), thumb_info.fmt);

        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Thumb(thumb_insn.clone()));

        (thumb_info.handler_fn)(self, sb, &thumb_insn)
    }

    #[cfg(not(feature = "arm7tdmi_dispatch_table"))]
    fn step_arm_exec(&mut self, insn: u32, sb: &mut SysBus) -> CpuAction {
        let arm_insn = ArmInstruction::decode(insn, self.pc.wrapping_sub(8));

        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Arm(arm_insn.clone()));

        self.exec_arm(sb, &arm_insn)
    }

    #[cfg(not(feature = "arm7tdmi_dispatch_table"))]
    fn step_thumb_exec(&mut self, insn: u16, sb: &mut SysBus) -> CpuAction {
        let thumb_insn = ThumbInstruction::decode(insn, self.pc.wrapping_sub(4));

        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Thumb(thumb_insn.clone()));

        self.exec_thumb(sb, &thumb_insn)
    }

    #[inline(always)]
    pub fn reload_pipeline16(&mut self, sb: &mut SysBus) {
        self.pipeline[0] = sb.read_16(self.pc) as u32;
        self.N_cycle16(sb, self.pc);
        self.advance_thumb();
        self.pipeline[1] = sb.read_16(self.pc) as u32;
        self.S_cycle16(sb, self.pc);
        self.advance_thumb();
    }

    #[inline(always)]
    pub fn reload_pipeline32(&mut self, sb: &mut SysBus) {
        self.pipeline[0] = sb.read_32(self.pc);
        self.N_cycle16(sb, self.pc);
        self.advance_arm();
        self.pipeline[1] = sb.read_32(self.pc);
        self.S_cycle16(sb, self.pc);
        self.advance_arm();
    }

    #[inline]
    pub(super) fn advance_thumb(&mut self) {
        self.pc = self.pc.wrapping_add(2)
    }

    #[inline]
    pub(super) fn advance_arm(&mut self) {
        self.pc = self.pc.wrapping_add(4)
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    pub fn step(&mut self, bus: &mut SysBus) {
        let pc = self.pc;

        match self.cpsr.state() {
            CpuState::ARM => {
                let fetched_now = bus.read_32(pc);
                let insn = self.pipeline[0];
                self.pipeline[0] = self.pipeline[1];
                self.pipeline[1] = fetched_now;
                let cond =
                    ArmCond::from_u32(insn.bit_range(28..32)).expect("invalid arm condition");
                if cond != ArmCond::AL {
                    if !self.check_arm_cond(cond) {
                        self.S_cycle32(bus, self.pc);
                        self.advance_arm();
                        return;
                    }
                }
                match self.step_arm_exec(insn, bus) {
                    CpuAction::AdvancePC => self.advance_arm(),
                    CpuAction::FlushPipeline => {}
                }
            }
            CpuState::THUMB => {
                let fetched_now = bus.read_16(pc);
                let insn = self.pipeline[0];
                self.pipeline[0] = self.pipeline[1];
                self.pipeline[1] = fetched_now as u32;
                match self.step_thumb_exec(insn as u16, bus) {
                    CpuAction::AdvancePC => self.advance_thumb(),
                    CpuAction::FlushPipeline => {}
                }
            }
        }
    }

    /// Get's the address of the next instruction that is going to be executed
    pub fn get_next_pc(&self) -> Addr {
        let insn_size = self.word_size() as u32;
        self.pc - 2 * insn_size
    }

    pub fn get_cpu_state(&self) -> CpuState {
        self.cpsr.state()
    }

    pub fn skip_bios(&mut self) {
        self.gpr_banked_r13[0] = 0x0300_7f00; // USR/SYS
        self.gpr_banked_r13[1] = 0x0300_7f00; // FIQ
        self.gpr_banked_r13[2] = 0x0300_7fa0; // IRQ
        self.gpr_banked_r13[3] = 0x0300_7fe0; // SVC
        self.gpr_banked_r13[4] = 0x0300_7f00; // ABT
        self.gpr_banked_r13[5] = 0x0300_7f00; // UND

        self.gpr[13] = 0x0300_7f00;
        self.pc = 0x0800_0000;

        self.cpsr.set(0x5f);
    }
}

#[cfg(feature = "debugger")]
impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
