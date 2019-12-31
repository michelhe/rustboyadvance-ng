use super::super::sysbus::SysBus;
use super::cpu::{Core, PipelineState};
use super::{CpuMode, CpuState};
use colored::*;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
/// Models a CPU exception, and maps to the relavnt entry in the exception vector
pub enum Exception {
    Reset = 0x00,
    UndefinedInstruction = 0x04,
    SoftwareInterrupt = 0x08,
    PrefatchAbort = 0x0c,
    DataAbort = 0x10,
    Reserved = 0x14,
    Irq = 0x18,
    Fiq = 0x1c,
}

impl Core {
    pub fn exception(&mut self, sb: &mut SysBus, e: Exception, lr: u32) {
        use Exception::*;
        let (new_mode, irq_disable, fiq_disable) = match e {
            Reset => (CpuMode::Supervisor, true, true),
            UndefinedInstruction => (CpuMode::Undefined, false, false),
            SoftwareInterrupt => (CpuMode::Supervisor, true, false),
            DataAbort => (CpuMode::Abort, false, false),
            PrefatchAbort => (CpuMode::Abort, false, false),
            Reserved => panic!("Cpu reserved exception"),
            Irq => (CpuMode::Irq, true, false),
            Fiq => (CpuMode::Fiq, true, true),
        };
        if self.trace_exceptions {
            println!(
                "{}: {:?}, pc: {:#x}, new_mode: {:?} old_mode: {:?}",
                "Exception".cyan(),
                e,
                self.pc,
                new_mode,
                self.cpsr.mode(),
            );
        }

        let new_bank = new_mode.bank_index();
        self.spsr_bank[new_bank] = self.cpsr;
        self.gpr_banked_r14[new_bank] = lr;
        self.change_mode(self.cpsr.mode(), new_mode);

        // Set appropriate CPSR bits
        self.cpsr.set_state(CpuState::ARM);
        self.cpsr.set_mode(new_mode);
        if irq_disable {
            self.cpsr.set_irq_disabled(true);
        }
        if fiq_disable {
            self.cpsr.set_fiq_disabled(true);
        }

        // Set PC to vector address
        self.pc = e as u32;
        self.flush_pipeline32(sb);
    }

    pub fn irq(&mut self, sb: &mut SysBus) {
        if self.pipeline_state != PipelineState::Execute {
            panic!("IRQ when pipeline refilling! {:?}", self.pipeline_state);
        }
        if !self.cpsr.irq_disabled() {
            let lr = self.get_next_pc() + 4;
            self.exception(sb, Exception::Irq, lr)
        }
    }

    pub fn software_interrupt(&mut self, sb: &mut SysBus, lr: u32, cmt: u32) {
        match self.cpsr.state() {
            CpuState::ARM => self.N_cycle32(sb, self.pc),
            CpuState::THUMB => self.N_cycle16(sb, self.pc),
        };
        if cmt == 0x55 {
            #[cfg(debug_assertions)]
            {
                println!("Special breakpoint detected!");
                host_breakpoint!();
            }
        } else {
            self.exception(sb, Exception::SoftwareInterrupt, lr);
        }
    }
}
