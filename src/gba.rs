/// Struct containing everything
///
use super::arm7tdmi::{Core, DecodedInstruction};
use super::cartridge::Cartridge;
use super::dma::DmaChannel;
use super::ioregs::consts::*;
use super::lcd::Lcd;
use super::sysbus::SysBus;

use super::{EmuIoDev, GBAResult};

#[derive(Debug)]
pub struct GameBoyAdvance {
    pub cpu: Core,
    pub sysbus: SysBus,

    // io devices
    pub lcd: Lcd,
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

            lcd: Lcd::new(),
            dma0: DmaChannel::new(REG_DMA0SAD, REG_DMA0DAD, REG_DMA0DAD),
            dma1: DmaChannel::new(REG_DMA1SAD, REG_DMA1DAD, REG_DMA1DAD),
            dma2: DmaChannel::new(REG_DMA2SAD, REG_DMA2DAD, REG_DMA2DAD),
            dma3: DmaChannel::new(REG_DMA3SAD, REG_DMA3DAD, REG_DMA3DAD),

            post_bool_flags: false,
        }
    }

    fn run_cpu_for_n_cycles(&mut self, n: usize) {
        let previous_cycles = self.cpu.cycles;
        loop {
            self.cpu.step_one(&mut self.sysbus).unwrap();
            if n > self.cpu.cycles - previous_cycles {
                break;
            }
        }
    }

    pub fn frame(&mut self) {
        for _ in 0..Lcd::DISPLAY_HEIGHT {
            self.run_cpu_for_n_cycles(Lcd::CYCLES_HDRAW);
            let _irq = self.lcd.set_hblank(&mut self.sysbus);
            self.run_cpu_for_n_cycles(Lcd::CYCLES_HBLANK);
        }
        let _irq = self.lcd.set_vblank(&mut self.sysbus);
        self.run_cpu_for_n_cycles(Lcd::CYCLES_VBLANK);
        self.lcd.render(&mut self.sysbus); // Currently not implemented
        self.lcd.set_hdraw();
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

        // let (lcd_cycles, _) = self.lcd.step(cycles, &mut self.sysbus);
        // cycles += lcd_cycles;

        Ok(executed_insn)
    }
}
