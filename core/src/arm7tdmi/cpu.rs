use serde::{Deserialize, Serialize};

pub use super::exception::Exception;

use super::{arm::ArmCond, psr::RegPSR, Addr, CpuMode, CpuState};

use crate::util::{Shared, WeakPointer};

use super::memory::{MemoryAccess, MemoryInterface};
use MemoryAccess::*;

use cfg_if::cfg_if;

#[cfg(feature = "debugger")]
use super::thumb::ThumbFormat;

#[cfg(feature = "debugger")]
use super::arm::ArmFormat;

#[cfg_attr(not(feature = "debugger"), repr(transparent))]
pub struct ThumbInstructionInfo<I: MemoryInterface> {
    pub handler_fn: fn(&mut Core<I>, insn: u16) -> CpuAction,
    #[cfg(feature = "debugger")]
    pub fmt: ThumbFormat,
}

#[cfg_attr(not(feature = "debugger"), repr(transparent))]
pub struct ArmInstructionInfo<I: MemoryInterface> {
    pub handler_fn: fn(&mut Core<I>, insn: u32) -> CpuAction,
    #[cfg(feature = "debugger")]
    pub fmt: ArmFormat,
}

cfg_if! {
    if #[cfg(feature = "debugger")] {
        use super::DecodedInstruction;
        use super::arm::ArmInstruction;
        use super::thumb::ThumbInstruction;
        use super::reg_string;
        use std::fmt;

        use ansi_term::{Colour, Style};
    } else {

    }
}

use bit::BitIndex;
use num::FromPrimitive;

pub enum CpuAction {
    AdvancePC(MemoryAccess),
    PipelineFlushed,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub(super) struct BankedRegisters {
    // r13 and r14 are banked for all modes. System&User mode share them
    pub(super) gpr_banked_r13: [u32; 6],
    pub(super) gpr_banked_r14: [u32; 6],
    // r8-r12 are banked for fiq mode
    pub(super) gpr_banked_old_r8_12: [u32; 5],
    pub(super) gpr_banked_fiq_r8_12: [u32; 5],
    pub(super) spsr_bank: [RegPSR; 6],
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedCpuState {
    pub pc: u32,
    pub gpr: [u32; 15],
    next_fetch_access: MemoryAccess,
    pipeline: [u32; 2],

    pub cpsr: RegPSR,
    pub(super) spsr: RegPSR,

    pub(super) banks: BankedRegisters,
}

#[derive(Clone, Debug)]
#[cfg(feature = "debugger")]
pub struct DebuggerState {
    pub last_executed: Option<DecodedInstruction>,
    /// store the gpr before executing an instruction to show diff in the Display impl
    pub gpr_previous: [u32; 15],
    pub breakpoints: Vec<u32>,
    pub verbose: bool,
    pub trace_opcodes: bool,
    pub trace_exceptions: bool,
}

#[cfg(feature = "debugger")]
impl Default for DebuggerState {
    fn default() -> DebuggerState {
        DebuggerState {
            last_executed: None,
            gpr_previous: [0; 15],
            breakpoints: Vec::new(),
            verbose: false,
            trace_opcodes: false,
            trace_exceptions: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Core<I: MemoryInterface> {
    pub pc: u32,
    pub(super) bus: Shared<I>,

    next_fetch_access: MemoryAccess,
    pipeline: [u32; 2],
    pub gpr: [u32; 15],

    pub cpsr: RegPSR,
    pub(super) spsr: RegPSR,

    pub(super) banks: BankedRegisters,

    #[cfg(feature = "debugger")]
    pub dbg: DebuggerState,
}

impl<I: MemoryInterface> Core<I> {
    pub fn new(bus: Shared<I>) -> Core<I> {
        let cpsr = RegPSR::new(0x0000_00D3);
        Core {
            bus,
            pc: 0,
            gpr: [0; 15],
            pipeline: [0; 2],
            next_fetch_access: MemoryAccess::NonSeq,
            cpsr,
            spsr: Default::default(),
            banks: BankedRegisters::default(),

            #[cfg(feature = "debugger")]
            dbg: DebuggerState::default(),
        }
    }

    pub fn weak_ptr(&mut self) -> WeakPointer<Core<I>> {
        WeakPointer::new(self as *mut Core<I>)
    }

    pub fn from_saved_state(bus: Shared<I>, state: SavedCpuState) -> Core<I> {
        Core {
            bus,

            pc: state.pc,
            cpsr: state.cpsr,
            gpr: state.gpr,
            banks: state.banks,
            spsr: state.spsr,

            pipeline: state.pipeline,
            next_fetch_access: state.next_fetch_access,

            // savestate does not keep debugger related information, so just reinitialize to default
            #[cfg(feature = "debugger")]
            dbg: DebuggerState::default(),
        }
    }

    pub fn save_state(&self) -> SavedCpuState {
        SavedCpuState {
            cpsr: self.cpsr,
            pc: self.pc,
            gpr: self.gpr.clone(),
            spsr: self.spsr,
            banks: self.banks.clone(),
            pipeline: self.pipeline.clone(),
            next_fetch_access: self.next_fetch_access,
        }
    }

    pub fn restore_state(&mut self, state: SavedCpuState) {
        self.pc = state.pc;
        self.cpsr = state.cpsr;
        self.gpr = state.gpr;
        self.spsr = state.spsr;
        self.banks = state.banks;
        self.pipeline = state.pipeline;
        self.next_fetch_access = state.next_fetch_access;
    }

    pub fn set_memory_interface(&mut self, i: Shared<I>) {
        self.bus = i;
    }

    #[cfg(feature = "debugger")]
    pub fn set_verbose(&mut self, v: bool) {
        self.dbg.verbose = v;
    }

    pub fn get_reg(&self, r: usize) -> u32 {
        match r {
            0..=14 => self.gpr[r],
            15 => self.pc,
            _ => panic!("invalid register {}", r),
        }
    }

    #[inline]
    /// Gets PC of the currently executed instruction in arm mode
    pub fn pc_arm(&self) -> u32 {
        self.pc.wrapping_sub(8)
    }

    #[inline]
    /// Gets PC of the currently executed instruction in thumb mode
    pub fn pc_thumb(&self) -> u32 {
        self.pc.wrapping_sub(4)
    }

    pub fn get_reg_user(&mut self, r: usize) -> u32 {
        match r {
            0..=7 => self.gpr[r],
            8..=12 => {
                if self.cpsr.mode() == CpuMode::Fiq {
                    self.gpr[r]
                } else {
                    self.banks.gpr_banked_old_r8_12[r - 8]
                }
            }
            13 => self.banks.gpr_banked_r13[0],
            14 => self.banks.gpr_banked_r14[0],
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
                    self.banks.gpr_banked_old_r8_12[r - 8] = val;
                }
            }
            13 => {
                self.banks.gpr_banked_r13[0] = val;
            }
            14 => {
                self.banks.gpr_banked_r14[0] = val;
            }
            _ => panic!("invalid register"),
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

        let banks = &mut self.banks;

        banks.spsr_bank[old_index] = self.spsr;
        banks.gpr_banked_r13[old_index] = self.gpr[13];
        banks.gpr_banked_r14[old_index] = self.gpr[14];

        self.spsr = banks.spsr_bank[new_index];
        self.gpr[13] = banks.gpr_banked_r13[new_index];
        self.gpr[14] = banks.gpr_banked_r14[new_index];

        if new_mode == CpuMode::Fiq {
            for r in 0..5 {
                banks.gpr_banked_old_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = banks.gpr_banked_fiq_r8_12[r];
            }
        } else if old_mode == CpuMode::Fiq {
            for r in 0..5 {
                banks.gpr_banked_fiq_r8_12[r] = self.gpr[r + 8];
                self.gpr[r + 8] = banks.gpr_banked_old_r8_12[r];
            }
        }
        self.cpsr.set_mode(new_mode);
    }

    /// Resets the cpu
    pub fn reset(&mut self) {
        self.exception(Exception::Reset, 0);
    }

    pub fn word_size(&self) -> usize {
        match self.cpsr.state() {
            CpuState::ARM => 4,
            CpuState::THUMB => 2,
        }
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

    #[inline(always)]
    pub(super) fn check_arm_cond(&self, cond: ArmCond) -> bool {
        use ArmCond::*;
        match cond {
            Invalid => {
                // TODO - we would normally want to panic here
                false
            }
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
        self.dbg.gpr_previous = self.get_registers();
        self.dbg.last_executed = Some(d);
    }

    fn step_arm_exec(&mut self, insn: u32) -> CpuAction {
        let hash = (((insn >> 16) & 0xff0) | ((insn >> 4) & 0xf)) as usize;
        let arm_info = &Self::ARM_LUT[hash];
        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Arm(ArmInstruction::new(
            insn,
            self.pc.wrapping_sub(8),
            arm_info.fmt,
        )));
        (arm_info.handler_fn)(self, insn)
    }

    fn step_thumb_exec(&mut self, insn: u16) -> CpuAction {
        let thumb_info = &Self::THUMB_LUT[(insn >> 6) as usize];
        #[cfg(feature = "debugger")]
        self.debugger_record_step(DecodedInstruction::Thumb(ThumbInstruction::new(
            insn,
            self.pc.wrapping_sub(4),
            thumb_info.fmt,
        )));
        (thumb_info.handler_fn)(self, insn)
    }

    /// 2S + 1N
    #[inline(always)]
    pub fn reload_pipeline16(&mut self) {
        self.pipeline[0] = self.load_16(self.pc, NonSeq) as u32;
        self.advance_thumb();
        self.pipeline[1] = self.load_16(self.pc, Seq) as u32;
        self.advance_thumb();
        self.next_fetch_access = Seq;
    }

    /// 2S + 1N
    #[inline(always)]
    pub fn reload_pipeline32(&mut self) {
        self.pipeline[0] = self.load_32(self.pc, NonSeq);
        self.advance_arm();
        self.pipeline[1] = self.load_32(self.pc, Seq);
        self.advance_arm();
        self.next_fetch_access = Seq;
    }

    #[inline]
    pub(super) fn advance_thumb(&mut self) {
        self.pc = self.pc.wrapping_add(2)
    }

    #[inline]
    pub(super) fn advance_arm(&mut self) {
        self.pc = self.pc.wrapping_add(4)
    }

    #[inline]
    pub fn get_decoded_opcode(&self) -> u32 {
        self.pipeline[0]
    }

    #[inline]
    pub fn get_prefetched_opcode(&self) -> u32 {
        self.pipeline[1]
    }

    /// Perform a pipeline step
    /// If an instruction was executed in this step, return it.
    #[inline]
    pub fn step(&mut self) {
        match self.cpsr.state() {
            CpuState::ARM => {
                let pc = self.pc & !3;

                let fetched_now = self.load_32(pc, self.next_fetch_access);
                let insn = self.pipeline[0];
                self.pipeline[0] = self.pipeline[1];
                self.pipeline[1] = fetched_now;
                let cond = ArmCond::from_u8(insn.bit_range(28..32) as u8)
                    .unwrap_or_else(|| unsafe { std::hint::unreachable_unchecked() });
                if cond != ArmCond::AL {
                    if !self.check_arm_cond(cond) {
                        self.advance_arm();
                        self.next_fetch_access = MemoryAccess::NonSeq;
                        return;
                    }
                }
                match self.step_arm_exec(insn) {
                    CpuAction::AdvancePC(access) => {
                        self.next_fetch_access = access;
                        self.advance_arm();
                    }
                    CpuAction::PipelineFlushed => {}
                }
            }
            CpuState::THUMB => {
                let pc = self.pc & !1;

                let fetched_now = self.load_16(pc, self.next_fetch_access);
                let insn = self.pipeline[0];
                self.pipeline[0] = self.pipeline[1];
                self.pipeline[1] = fetched_now as u32;
                match self.step_thumb_exec(insn as u16) {
                    CpuAction::AdvancePC(access) => {
                        self.advance_thumb();
                        self.next_fetch_access = access;
                    }
                    CpuAction::PipelineFlushed => {}
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
        self.banks.gpr_banked_r13[0] = 0x0300_7f00; // USR/SYS
        self.banks.gpr_banked_r13[1] = 0x0300_7f00; // FIQ
        self.banks.gpr_banked_r13[2] = 0x0300_7fa0; // IRQ
        self.banks.gpr_banked_r13[3] = 0x0300_7fe0; // SVC
        self.banks.gpr_banked_r13[4] = 0x0300_7f00; // ABT
        self.banks.gpr_banked_r13[5] = 0x0300_7f00; // UND

        self.gpr[13] = 0x0300_7f00;
        self.pc = 0x0800_0000;

        self.cpsr.set(0x5f);
    }
}

#[cfg(feature = "debugger")]
impl<I: MemoryInterface> fmt::Display for Core<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ARM7TDMI Core Status:")?;
        writeln!(f, "\tCPSR: {}", self.cpsr)?;
        writeln!(f, "\tGeneral Purpose Registers:")?;
        let reg_normal_style = Style::new().bold();
        let reg_dirty_style = Colour::Black.bold().on(Colour::Yellow);
        let gpr = self.get_registers();
        for i in 0..15 {
            let mut reg_name = reg_string(i).to_string();
            reg_name.make_ascii_uppercase();

            let style = if gpr[i] != self.dbg.gpr_previous[i] {
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

include!(concat!(env!("OUT_DIR"), "/arm_lut.rs"));
include!(concat!(env!("OUT_DIR"), "/thumb_lut.rs"));
