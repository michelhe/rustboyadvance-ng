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
    lcd: Lcd,
    dma0: DmaChannel,
    dma1: DmaChannel,
    dma2: DmaChannel,
    dma3: DmaChannel,

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

    pub fn step(&mut self) -> GBAResult<DecodedInstruction> {
        let previous_cycles = self.cpu.cycles;
        let decoded = self.cpu.step_one(&mut self.sysbus)?;

        // drop interrupts at the moment
        let cycles = self.cpu.cycles - previous_cycles;
        let (dma_cycles, _) = self.dma0.step(cycles, &mut self.sysbus);
        let cycles = cycles + dma_cycles;

        let cycles = self.cpu.cycles - previous_cycles;
        let (dma_cycles, _) = self.dma1.step(cycles, &mut self.sysbus);
        let cycles = cycles + dma_cycles;

        let cycles = self.cpu.cycles - previous_cycles;
        let (dma_cycles, _) = self.dma2.step(cycles, &mut self.sysbus);
        let cycles = cycles + dma_cycles;

        let cycles = self.cpu.cycles - previous_cycles;
        let (dma_cycles, _) = self.dma3.step(cycles, &mut self.sysbus);
        let cycles = cycles + dma_cycles;

        let (lcd_cycles, _) = self.lcd.step(cycles, &mut self.sysbus);

        Ok(decoded) // return the decoded instruction for the debugger
    }
}
