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

pub struct GameBoyAdvance {
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
    pub fn new(cpu: Core, bios_rom: Vec<u8>, gamepak: Cartridge) -> GameBoyAdvance {
        let sysbus = SysBus::new(bios_rom, gamepak);

        GameBoyAdvance {
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

    fn emulate_n_cycles(&mut self, mut n: usize) {
        let mut cycles = 0;
        loop {
            let previous_cycles = self.cpu.cycles;
            self.cpu.step_one(&mut self.sysbus).unwrap();
            let new_cycles = self.cpu.cycles - previous_cycles;

            self.gpu.step(new_cycles, &mut self.sysbus);
            cycles += new_cycles;

            if n <= cycles {
                break;
            }
        }
    }

    pub fn frame(&mut self) {
        while self.gpu.state == GpuState::VBlank {
            self.emulate();
        }
        while self.gpu.state != GpuState::VBlank {
            self.emulate();
        }
    }

    pub fn emulate(&mut self) {
        let previous_cycles = self.cpu.cycles;
        self.cpu.step(&mut self.sysbus).unwrap();
        let cycles = self.cpu.cycles - previous_cycles;
        self.gpu.step(cycles, &mut self.sysbus);
    }

    fn interrupts_disabled(&self) -> bool {
        self.sysbus.ioregs.read_reg(REG_IME) & 1 == 0
    }

    fn request_irq(&mut self, irq: Interrupt) {
        // if self.interrupts_disabled() {
        //     return;
        // }
        // let irq_bit = irq as usize;
        // let reg_ie = self.sysbus.ioregs.read_reg(REG_IE);
        // if reg_ie & (1 << irq_bit) != 0 {
        //     println!("entering {:?}", irq);
        //     self.cpu.exception(Exception::Irq);
        // }
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

        /* let (_, irq) = */
        self.gpu.step(cycles, &mut self.sysbus);
        // if let Some(irq) = irq {
        //     self.request_irq(irq);
        // }
        // cycles += lcd_cycles;

        Ok(executed_insn)
    }
}
