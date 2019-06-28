use super::{cpu::Core, CpuMode, CpuState};

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

impl From<Exception> for CpuMode {
    /// Return cpu mode upon entry
    fn from(e: Exception) -> CpuMode {
        use Exception::*;
        match e {
            Reset | SoftwareInterrupt | Reserved => CpuMode::Supervisor,
            PrefatchAbort | DataAbort => CpuMode::Abort,
            UndefinedInstruction => CpuMode::Undefined,
            Irq => CpuMode::Irq,
            Fiq => CpuMode::Fiq,
        }
    }
}

impl Core {
    pub fn exception(&mut self, e: Exception) {
        let vector = e as u32;
        let new_mode = CpuMode::from(e);
        if self.verbose {
            println!("{}: {:?}, new_mode: {:?}", "Exception".cyan(), e, new_mode);
        }

        self.change_mode(new_mode);
        // Set appropriate CPSR bits
        self.cpsr.set_state(CpuState::ARM);
        self.cpsr.set_mode(new_mode);
        self.cpsr.set_irq_disabled(true);
        let disabled_fiq = e == Exception::Reset || e == Exception::Fiq;
        self.cpsr.set_fiq_disabled(disabled_fiq);

        // Set PC to vector address
        self.pc = vector;
    }
}
