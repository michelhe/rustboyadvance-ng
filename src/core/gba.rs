/// Struct containing everything
use super::arm7tdmi::{exception::Exception, Core, CpuState, DecodedInstruction};
use super::cartridge::Cartridge;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sysbus::SysBus;

use super::GBAResult;
use super::SyncedIoDevice;
use crate::backend::*;

pub struct GameBoyAdvance {
    backend: Box<dyn EmulatorBackend>,
    pub cpu: Core,
    pub sysbus: Box<SysBus>,
}

impl GameBoyAdvance {
    pub fn new(
        cpu: Core,
        bios_rom: Vec<u8>,
        gamepak: Cartridge,
        backend: Box<dyn EmulatorBackend>,
    ) -> GameBoyAdvance {
        let io = IoDevices::new();
        GameBoyAdvance {
            backend: backend,
            cpu: cpu,
            sysbus: Box::new(SysBus::new(io, bios_rom, gamepak)),
        }
    }

    pub fn frame(&mut self) {
        self.update_key_state();
        while self.sysbus.io.gpu.state != GpuState::VBlank {
            self.step_new();
        }
        while self.sysbus.io.gpu.state == GpuState::VBlank {
            self.step_new();
        }
    }

    fn update_key_state(&mut self) {
        self.sysbus.io.keyinput = self.backend.get_key_state();
    }

    // TODO deprecate
    pub fn step(&mut self) -> GBAResult<DecodedInstruction> {
        let previous_cycles = self.cpu.cycles;
        let executed_insn = self.cpu.step_one(&mut self.sysbus)?;
        let cycles = self.cpu.cycles - previous_cycles;
        Ok(executed_insn)
    }

    pub fn step_new(&mut self) {
        let mut irqs = IrqBitmask(0);
        let previous_cycles = self.cpu.cycles;

        // // I hate myself for doing this, but rust left me no choice.
        let io = unsafe {
            let ptr = &mut *self.sysbus as *mut SysBus;
            &mut (*ptr).io as &mut IoDevices
        };

        if !io.dmac.perform_work(&mut self.sysbus, &mut irqs) {
            if io.intc.irq_pending() {
                self.cpu.irq(&mut self.sysbus);
            }
            self.cpu.step(&mut self.sysbus).unwrap();
        }

        let cycles = self.cpu.cycles - previous_cycles;

        io.timers.step(cycles, &mut self.sysbus, &mut irqs);
        if let Some(new_gpu_state) = io.gpu.step(cycles, &mut self.sysbus, &mut irqs) {
            match new_gpu_state {
                GpuState::VBlank => {
                    self.backend.render(io.gpu.get_framebuffer());
                    io.dmac.notify_vblank();
                }
                GpuState::HBlank => io.dmac.notify_hblank(),
                _ => {}
            }
        }

        io.intc.request_irqs(irqs);
    }
}
