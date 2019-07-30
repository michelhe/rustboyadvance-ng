/// Struct containing everything
///
use super::arm7tdmi::{exception::*, Core, DecodedInstruction};
use super::cartridge::Cartridge;
use super::dma::DmaChannel;
use super::gpu::*;
use super::interrupt::*;
use super::ioregs::consts::*;
use super::sysbus::SysBus;
use super::EmuIoDev;
use super::{GBAError, GBAResult};
use crate::backend::*;

use crate::bit::BitIndex;

pub struct GameBoyAdvance {
    backend: Box<EmulatorBackend>,
    pub cpu: Core,
    pub sysbus: SysBus,

    // io devices
    pub gpu: Gpu,
    pub dma0: DmaChannel,
    pub dma1: DmaChannel,
    pub dma2: DmaChannel,
    pub dma3: DmaChannel,

    post_bool_flags: bool,
}

impl GameBoyAdvance {
    pub fn new(
        cpu: Core,
        bios_rom: Vec<u8>,
        gamepak: Cartridge,
        backend: Box<EmulatorBackend>,
    ) -> GameBoyAdvance {
        let sysbus = SysBus::new(bios_rom, gamepak);

        GameBoyAdvance {
            backend: backend,
            cpu: cpu,
            sysbus: sysbus,

            gpu: Gpu::new(),
            dma0: DmaChannel::new(REG_DMA0SAD, REG_DMA0DAD, REG_DMA0DAD),
            dma1: DmaChannel::new(REG_DMA1SAD, REG_DMA1DAD, REG_DMA1DAD),
            dma2: DmaChannel::new(REG_DMA2SAD, REG_DMA2DAD, REG_DMA2DAD),
            dma3: DmaChannel::new(REG_DMA3SAD, REG_DMA3DAD, REG_DMA3DAD),

            post_bool_flags: false,
        }
    }

    pub fn frame(&mut self) {
        self.update_key_state();
        while self.gpu.state != GpuState::VBlank {
            self.emulate();
        }
        self.backend.render(self.gpu.render());
        while self.gpu.state == GpuState::VBlank {
            self.emulate();
        }
    }

    fn update_key_state(&mut self) {
        let keyinput = self.backend.get_key_state();
        self.sysbus.ioregs.write_reg(REG_KEYINPUT, keyinput);
    }

    pub fn emulate(&mut self) {
        let previous_cycles = self.cpu.cycles;
        self.cpu.step(&mut self.sysbus).unwrap();
        let cycles = self.cpu.cycles - previous_cycles;
        let (_, irq) = self.gpu.step(cycles, &mut self.sysbus);
        if let Some(irq) = irq {
            self.request_irq(irq);
        }
    }

    fn interrupts_disabled(&self) -> bool {
        self.cpu.cpsr.irq_disabled() | (self.sysbus.ioregs.read_reg(REG_IME) & 1 == 0)
    }

    fn request_irq(&mut self, irq: Interrupt) {
        if self.interrupts_disabled() {
            return;
        }
        let irq_bit_index = irq as usize;
        let reg_ie = self.sysbus.ioregs.read_reg(REG_IE);
        if reg_ie.bit(irq_bit_index) {
            self.sysbus
                .ioregs
                .write_reg(REG_IF, (1 << irq_bit_index) as u16);
            self.cpu.exception(Exception::Irq);
        }
    }

    pub fn step(&mut self) -> GBAResult<DecodedInstruction> {
        let previous_cycles = self.cpu.cycles;
        let executed_insn = self.cpu.step_one(&mut self.sysbus)?;

        let mut cycles = self.cpu.cycles - previous_cycles;

        // // drop interrupts at the moment

        // let (dma_cycles, _) = self.dma0.step(cycles, &mut self.sysbus);
        // cycles += dma_cycles;

        // let (dma_cycles, _) = self.dma1.step(cycles, &mut self.sysbus);
        // cycles += dma_cycles;

        // let (dma_cycles, _) = self.dma2.step(cycles, &mut self.sysbus);
        // cycles += dma_cycles;

        // let (dma_cycles, _) = self.dma3.step(cycles, &mut self.sysbus);
        // cycles += dma_cycles;

        let (_, irq) = self.gpu.step(cycles, &mut self.sysbus);
        if let Some(irq) = irq {
            self.request_irq(irq);
        }

        Ok(executed_insn)
    }
}
