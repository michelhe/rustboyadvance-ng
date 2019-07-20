pub mod arm7tdmi;
pub mod cartridge;
pub mod gpu;
pub mod sysbus;
pub use sysbus::SysBus;
pub mod interrupt;
pub mod ioregs;
pub use interrupt::Interrupt;
pub mod gba;
pub use gba::GameBoyAdvance;
pub mod dma;
pub mod palette;

use crate::debugger;

pub trait EmuIoDev {
    fn step(&mut self, cycles: usize, sysbus: &mut SysBus) -> (usize, Option<Interrupt>);
}

#[derive(Debug)]
pub enum GBAError {
    IO(::std::io::Error),
    CpuError(arm7tdmi::CpuError),
    DebuggerError(debugger::DebuggerError),
}

pub type GBAResult<T> = Result<T, GBAError>;

impl From<::std::io::Error> for GBAError {
    fn from(err: ::std::io::Error) -> GBAError {
        GBAError::IO(err)
    }
}

impl From<arm7tdmi::CpuError> for GBAError {
    fn from(err: arm7tdmi::CpuError) -> GBAError {
        GBAError::CpuError(err)
    }
}

impl From<debugger::DebuggerError> for GBAError {
    fn from(err: debugger::DebuggerError) -> GBAError {
        GBAError::DebuggerError(err)
    }
}
