use super::arm7tdmi::Bus;
use super::ioregs::consts::*;
use super::palette::{Palette, Rgb15};
use super::*;

use crate::bit::BitIndex;
use crate::num::FromPrimitive;

#[derive(Debug, Primitive)]
enum BGMode {
    BGMode0 = 0,
    BGMode1 = 1,
    BGMode2 = 2,
    BGMode3 = 3,
    BGMode4 = 4,
    BGMode5 = 5,
}

#[derive(Debug)]
pub struct DisplayControl {
    bg_mode: BGMode,
    display_frame: usize,
    hblank_interval_free: bool,
    obj_character_vram_mapping: bool, // true - 1 dimentional, false - 2 dimentional
    forced_blank: bool,
    disp_bg: [bool; 4],
    disp_obj: bool,
    disp_window0: bool,
    disp_window1: bool,
    disp_obj_window: bool,
}

impl From<u16> for DisplayControl {
    fn from(v: u16) -> Self {
        DisplayControl {
            bg_mode: BGMode::from_u8(v.bit_range(0..3) as u8).unwrap(),
            // bit 3 is unused
            display_frame: v.bit(4) as usize,
            hblank_interval_free: v.bit(5),
            obj_character_vram_mapping: v.bit(6),
            forced_blank: v.bit(7),
            disp_bg: [v.bit(8), v.bit(9), v.bit(10), v.bit(11)],
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
    raw_value: u16,
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
            vcount_setting: v.bit_range(8..16) as u8,
            raw_value: v,
        }
    }
}

#[derive(Debug)]
pub struct BgControl {
    bg_priority: u8,
    character_base_block: u8,
    moasic: bool,
    colors_palettes: bool, // 0=16/16, 1=256/1)
    screen_base_block: u8,
    wraparound: bool,
    screen_width: usize,
    screen_height: usize,
}

impl From<u16> for BgControl {
    fn from(v: u16) -> Self {
        let (width, height) = match v.bit_range(14..16) {
            0 => (256, 256),
            1 => (512, 256),
            2 => (256, 512),
            3 => (512, 512),
            _ => unreachable!(),
        };
        BgControl {
            bg_priority: v.bit_range(0..2) as u8,
            character_base_block: v.bit_range(2..3) as u8,
            moasic: v.bit(6),
            colors_palettes: v.bit(7), // 0=16/16, 1=256/1)
            screen_base_block: v.bit_range(8..13) as u8,
            wraparound: v.bit(13),
            screen_width: width,
            screen_height: height,
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum LcdState {
    HDraw = 0,
    HBlank,
    VBlank,
}
impl Default for LcdState {
    fn default() -> LcdState {
        LcdState::HDraw
    }
}
use LcdState::*;

#[derive(Debug, Default)]
pub struct Lcd {
    cycles: usize,
    state: LcdState,
    current_scanline: usize, // VCOUNT
}

impl Lcd {
    pub const DISPLAY_WIDTH: usize = 240;
    pub const DISPLAY_HEIGHT: usize = 160;

    pub const CYCLES_PIXEL: usize = 4;
    pub const CYCLES_HDRAW: usize = 960;
    pub const CYCLES_HBLANK: usize = 272;
    pub const CYCLES_SCANLINE: usize = 1232;
    pub const CYCLES_VDRAW: usize = 197120;
    pub const CYCLES_VBLANK: usize = 83776;

    pub fn new() -> Lcd {
        Lcd {
            state: HDraw,
            ..Default::default()
        }
    }

    fn palette(&self, sysbus: &SysBus) -> Palette {
        Palette::from(sysbus.get_bytes(0x0500_0000))
    }

    fn update_regs(&self, dispstat: DisplayStatus, sysbus: &mut SysBus) {
        let mut v = dispstat.raw_value;
        v.set_bit(0, dispstat.vblank_flag);
        v.set_bit(1, dispstat.hblank_flag);
        v.set_bit(2, dispstat.vcount_flag);
        sysbus.ioregs.write_reg(REG_DISPSTAT, v);
    }

    pub fn set_hblank(&mut self, sysbus: &mut SysBus) -> Option<Interrupt> {
        let dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));
        let mut v = dispstat.raw_value;
        v.set_bit(1, true);
        self.state = HBlank;
        sysbus.ioregs.write_reg(REG_DISPSTAT, v);

        if dispstat.hblank_irq_enable {
            Some(Interrupt::LCD_HBlank)
        } else {
            None
        }
    }

    pub fn set_vblank(&mut self, sysbus: &mut SysBus) -> Option<Interrupt> {
        let dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));
        let mut v = dispstat.raw_value;
        v.set_bit(1, false);
        v.set_bit(0, true);
        self.state = HBlank;
        sysbus.ioregs.write_reg(REG_DISPSTAT, v);

        if dispstat.vblank_irq_enable {
            Some(Interrupt::LCD_VBlank)
        } else {
            None
        }
    }

    pub fn set_hdraw(&mut self) {
        self.state = HDraw;
    }

    fn bgcnt(&self, sysbus: &SysBus, bg: u32) -> BgControl {
        BgControl::from(sysbus.ioregs.read_reg(REG_BG0CNT + 2 * bg))
    }

    pub fn render(&mut self, sysbus: &SysBus) {
        // let dispcnt = DisplayControl::from(sysbus.ioregs.read_reg(REG_DISPCNT));
        // let dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

        // TODO - redner
    }
}

// *TODO* Running the Lcd step by step causes a massive performance impact, so for now not treat it as an emulated IO device.
//
// impl EmuIoDev for Lcd {
//     fn step(&mut self, cycles: usize, sysbus: &mut SysBus) -> (usize, Option<Interrupt>) {
//         self.cycles += cycles;

//         sysbus
//             .ioregs
//             .write_reg(REG_VCOUNT, self.current_scanline as u16);
//         let mut dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

//         dispstat.vcount_flag = dispstat.vcount_setting as usize == self.current_scanline;
//         if dispstat.vcount_irq_enable {
//             panic!("VCOUNT IRQ NOT IMPL");
//         }

//         match self.state {
//             HDraw => {
//                 if self.cycles > Lcd::CYCLES_HDRAW {
//                     self.current_scanline += 1;
//                     self.cycles -= Lcd::CYCLES_HDRAW;

//                     let (new_state, irq) = if self.current_scanline < Lcd::DISPLAY_HEIGHT {
//                         // HBlank
//                         dispstat.hblank_flag = true;
//                         let irq = if dispstat.hblank_irq_enable {
//                             Some(Interrupt::LCD_HBlank)
//                         } else {
//                             None
//                         };
//                         (HBlank, irq)
//                     } else {
//                         dispstat.vblank_flag = true;
//                         let irq = if dispstat.vblank_irq_enable {
//                             Some(Interrupt::LCD_HBlank)
//                         } else {
//                             None
//                         };
//                         (VBlank, irq)
//                     };
//                     self.state = new_state;
//                     self.update_regs(dispstat, sysbus);
//                     return (0, irq);
//                 }
//             }
//             HBlank => {
//                 if self.cycles > Lcd::CYCLES_HBLANK {
//                     self.cycles -= Lcd::CYCLES_HBLANK;
//                     self.state = HDraw;
//                     dispstat.hblank_flag = false;
//                     self.update_regs(dispstat, sysbus);
//                     return (0, None);
//                 }
//             }
//             VBlank => {
//                 if self.cycles > Lcd::CYCLES_VBLANK {
//                     self.cycles -= Lcd::CYCLES_VBLANK;
//                     self.state = HDraw;
//                     dispstat.vblank_flag = false;
//                     self.current_scanline = 0;
//                     self.update_regs(dispstat, sysbus);
//                     return (0, None);
//                 }
//             }
//         }

//         // let mut dispcnt = DisplayControl::from(sysbus.ioregs.read_reg(REG_DISPCNT));
//         // let mut dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

//         // TODO
//         (0, None)
//     }
// }
