use super::cpu::Core;
use super::memory::MemoryInterface;
use super::{CpuMode, CpuState};

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

impl<I: MemoryInterface> Core<I> {
    pub fn exception(&mut self, e: Exception, lr: u32) {
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

        #[cfg(feature = "debugger")]
        {
            if self.dbg.trace_exceptions {
                trace!("exception {:?} lr={:x} new_mode={:?}", e, lr, new_mode);
            }
        }

        let new_bank = new_mode.bank_index();
        self.banks.spsr_bank[new_bank] = self.cpsr;
        self.banks.gpr_banked_r14[new_bank] = lr;
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
        self.reload_pipeline32();
    }

    #[inline]
    pub fn irq(&mut self) {
        if !self.cpsr.irq_disabled() {
            let lr = self.get_next_pc() + 4;
            self.exception(Exception::Irq, lr);
        }
    }

    #[inline]
    pub fn software_interrupt(&mut self, lr: u32, _cmt: u32) {
        self.exception(Exception::SoftwareInterrupt, lr);
    }
}
