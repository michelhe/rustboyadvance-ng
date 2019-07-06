use super::arm7tdmi::{Core, DecodedInstruction};
use super::cartridge::Cartridge;
use super::lcd::Lcd;
use super::sysbus::SysBus;
/// Struct containing everything
///
use super::{EmuIoDev, GBAResult};

#[derive(Debug)]
pub struct GameBoyAdvance {
    pub cpu: Core,
    pub sysbus: SysBus,

    // io devices
    lcd: Lcd,

    post_bool_flags: bool,
}

impl GameBoyAdvance {
    pub fn new(cpu: Core, bios_rom: Vec<u8>, gamepak: Cartridge) -> GameBoyAdvance {
        let sysbus = SysBus::new(bios_rom, gamepak);

        GameBoyAdvance {
            cpu: cpu,
            sysbus: sysbus,

            lcd: Lcd::new(),

            post_bool_flags: false,
        }
    }

    pub fn step(&mut self) -> GBAResult<DecodedInstruction> {
        let previous_cycles = self.cpu.cycles;
        let decoded = self.cpu.step_one(&mut self.sysbus)?;
        let cycles = self.cpu.cycles - previous_cycles;

        self.lcd.step(cycles, &mut self.sysbus); // drop interrupts at the moment

        Ok(decoded) // return the decoded instruction for the debugger
    }
}
