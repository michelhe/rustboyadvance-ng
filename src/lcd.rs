use super::ioregs::consts::*;
use super::*;

use crate::bit::BitIndex;
use crate::num::FromPrimitive;

#[derive(Debug, Primitive)]
enum BGMode {
    Mode0 = 0,
    Mode1 = 1,
    Mode2 = 2,
    Mode3 = 3,
    Mode4 = 4,
    Mode5 = 5,
}

#[derive(Debug)]
pub struct DisplayControl {
    bg_mode: BGMode,
    display_frame: usize,
    hblank_interval_free: bool,
    obj_character_vram_mapping: bool, // true - 1 dimentional, false - 2 dimentional
    forced_blank: bool,
    disp_bg0: bool,
    disp_bg1: bool,
    disp_bg2: bool,
    disp_bg3: bool,
    disp_obj: bool,
    disp_window0: bool,
    disp_window1: bool,
    disp_obj_window: bool,
}

impl From<u16> for DisplayControl {
    fn from(v: u16) -> Self {
        DisplayControl {
            bg_mode: BGMode::from_u8(v.bit_range(0..2) as u8).unwrap(),
            // bit 3 is unused
            display_frame: v.bit(4) as usize,
            hblank_interval_free: v.bit(5),
            obj_character_vram_mapping: v.bit(6),
            forced_blank: v.bit(7),
            disp_bg0: v.bit(8),
            disp_bg1: v.bit(9),
            disp_bg2: v.bit(10),
            disp_bg3: v.bit(11),
            disp_obj: v.bit(12),
            disp_window0: v.bit(13),
            disp_window1: v.bit(14),
            disp_obj_window: v.bit(15),
        }
    }
}

#[derive(Debug)]
pub struct DisplayStatus {
    vblank_flag: bool,
    hblank_flag: bool,
    vcount_flag: bool,
    vblank_irq_enable: bool,
    hblank_irq_enable: bool,
    vcount_irq_enable: bool,
    vcount_setting: u8,
}

impl From<u16> for DisplayStatus {
    fn from(v: u16) -> Self {
        DisplayStatus {
            vblank_flag: v.bit(0),
            hblank_flag: v.bit(1),
            vcount_flag: v.bit(2),
            vblank_irq_enable: v.bit(3),
            hblank_irq_enable: v.bit(4),
            vcount_irq_enable: v.bit(5),
            // bits 6-7 are unused in GBA
            vcount_setting: v.bit_range(8..15) as u8,
        }
    }
}

#[derive(Debug, Default)]
pub struct Lcd {
    cycles: usize,
    current_scanline: usize, // VCOUNT
}

impl Lcd {
    const DISPLAY_WIDTH: usize = 240;
    const DISPLAY_HEIGHT: usize = 160;

    pub fn new() -> Lcd {
        Lcd {
            ..Default::default()
        }
    }
}

impl EmuIoDev for Lcd {
    fn step(&mut self, cycles: usize, sysbus: &mut SysBus) -> Option<Interrupt> {
        self.cycles += cycles;

        let mut result = None;
        let mut dispcnt = DisplayControl::from(sysbus.ioregs.read_reg(REG_DISPCNT));
        let mut dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

        // TODO

        result
    }
}
