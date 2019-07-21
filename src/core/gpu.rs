use super::arm7tdmi::{Addr, Bus};
use super::ioregs::consts::*;
use super::palette::{Palette, PixelFormat, Rgb15};
use super::*;

use crate::bit::BitIndex;
use crate::num::FromPrimitive;

const VRAM_ADDR: Addr = 0x0600_0000;

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
    palette256: bool, // 0=16/16, 1=256/1)
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
            character_base_block: v.bit_range(2..4) as u8,
            moasic: v.bit(6),
            palette256: v.bit(7),
            screen_base_block: v.bit_range(8..13) as u8,
            wraparound: v.bit(13),
            screen_width: width,
            screen_height: height,
        }
    }
}

impl BgControl {
    pub fn char_block(&self) -> Addr {
        VRAM_ADDR + (self.character_base_block as u32) * 0x4000
    }

    pub fn screen_block(&self) -> Addr {
        VRAM_ADDR + (self.screen_base_block as u32) * 0x800
    }

    pub fn tile_format(&self) -> (u32, PixelFormat) {
        if self.palette256 {
            (2 * Gpu::TILE_SIZE, PixelFormat::BPP8)
        } else {
            (Gpu::TILE_SIZE, PixelFormat::BPP4)
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum GpuState {
    HDraw = 0,
    HBlank,
    VBlank,
}
impl Default for GpuState {
    fn default() -> GpuState {
        GpuState::HDraw
    }
}
use GpuState::*;

pub struct Gpu {
    cycles: usize,
    pub pixeldata: [Rgb15; 512 * 512],
    pub state: GpuState,
    pub current_scanline: usize, // VCOUNT
}

impl Gpu {
    pub const DISPLAY_WIDTH: usize = 240;
    pub const DISPLAY_HEIGHT: usize = 160;

    pub const CYCLES_PIXEL: usize = 4;
    pub const CYCLES_HDRAW: usize = 960;
    pub const CYCLES_HBLANK: usize = 272;
    pub const CYCLES_SCANLINE: usize = 1232;
    pub const CYCLES_VDRAW: usize = 197120;
    pub const CYCLES_VBLANK: usize = 83776;

    pub const TILE_SIZE: u32 = 0x20;

    pub fn new() -> Gpu {
        Gpu {
            state: HDraw,
            current_scanline: 0,
            cycles: 0,
            pixeldata: [Rgb15::from(0); 512 * 512],
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
        self.state = VBlank;
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

    fn bgcnt(&self, bg: u32, sysbus: &SysBus) -> BgControl {
        BgControl::from(sysbus.ioregs.read_reg(REG_BG0CNT + 2 * bg))
    }

    fn bgofs(&self, bg: u32, sysbus: &SysBus) -> (u32, u32) {
        let hofs = (sysbus.ioregs.read_reg(REG_BG0HOFS + 4 * bg) & 0x1ff) as u32;
        let vofs = (sysbus.ioregs.read_reg(REG_BG0VOFS + 4 * bg) & 0x1ff) as u32;
        (hofs, vofs)
    }

    /// helper method that reads the palette index from a base address and x + y
    pub fn read_pixel_index(
        &self,
        sysbus: &SysBus,
        addr: Addr,
        x: u32,
        y: u32,
        width: u32,
        format: PixelFormat,
    ) -> usize {
        match format {
            PixelFormat::BPP4 => {
                let byte = sysbus.read_8(addr + width * y + x / 2);
                if x & 1 != 0 {
                    (byte >> 4) as usize
                } else {
                    (byte & 0xf) as usize
                }
            }
            PixelFormat::BPP8 => sysbus.read_8(addr + width * y + x) as usize,
        }
    }

    pub fn get_palette_color(&self, sysbus: &SysBus, index: u32, palette_index: u32) -> Rgb15 {
        sysbus
            .read_16(0x0500_0000 + 2 * index + 0x20 * palette_index)
            .into()
    }

    fn scanline_mode0(&mut self, bg: u32, sysbus: &mut SysBus) {
        let bgcnt = self.bgcnt(bg, sysbus);
        let tileset_base = bgcnt.char_block();
        let tilemap_base = bgcnt.screen_block();
        let (tile_size, pixel_format) = bgcnt.tile_format();

        let tiles_per_row = bgcnt.screen_width / 8;

        let mut px = 0;
        let py = self.current_scanline;
        let tile_y = py % 8;

        for tile in 0..tiles_per_row {
            let tile_y = py % 8;

            let map_addr = tilemap_base + (tile as u32) * 2;
            let entry = TileMapEntry::from(sysbus.read_16(map_addr));
            let tile_addr = tileset_base + entry.tile_index * tile_size;

            for tile_x in 0..=7 {
                let color = match pixel_format {
                    PixelFormat::BPP4 => {
                        let index = self.read_pixel_index(
                            sysbus,
                            tile_addr,
                            tile_x,
                            tile_y as u32,
                            4,
                            pixel_format,
                        );
                        self.get_palette_color(sysbus, index as u32, entry.palette_bank as u32)
                    }
                    PixelFormat::BPP8 => {
                        let index = self.read_pixel_index(
                            sysbus,
                            tile_addr,
                            tile_x,
                            tile_y as u32,
                            8,
                            pixel_format,
                        );
                        self.get_palette_color(sysbus, index as u32, 0)
                    }
                };
                self.pixeldata[((px + tile_x) as usize) + py * 512] = color;
            }
            px += 8;
            if px == bgcnt.screen_width as u32 {
                return;
            }
        }
    }

    fn scanline_mode3(&mut self, bg: u32, sb: &mut SysBus) {
        let y = self.current_scanline;

        for x in 0..Self::DISPLAY_WIDTH {
            let pixel_index = x + y * Self::DISPLAY_WIDTH;
            let pixel_addr = 0x0600_0000 + 2 * (pixel_index as u32);
            self.pixeldata[x + y * 512] = sb.read_16(pixel_addr).into();
        }
    }

    fn scanline_mode4(&mut self, bg: u32, dispcnt: &DisplayControl, sysbus: &mut SysBus) {
        let page: u32 = match dispcnt.display_frame {
            0 => 0x0600_0000,
            1 => 0x0600_a000,
            _ => unreachable!(),
        };

        let y = self.current_scanline;

        for x in 0..Self::DISPLAY_WIDTH {
            let bitmap_index = x + y * Self::DISPLAY_WIDTH;
            let bitmap_addr = page + (bitmap_index as u32);
            let index = sysbus.read_8(bitmap_addr as Addr) as u32;
            self.pixeldata[x + y * 512] = self.get_palette_color(sysbus, index, 0);
        }
    }

    pub fn scanline(&mut self, sysbus: &mut SysBus) {
        let dispcnt = DisplayControl::from(sysbus.ioregs.read_reg(REG_DISPCNT));

        match dispcnt.bg_mode {
            BGMode::BGMode0 | BGMode::BGMode2 => {
                for bg in 0..3 {
                    if dispcnt.disp_bg[bg] {
                        self.scanline_mode0(bg as u32, sysbus);
                    }
                }
            }
            BGMode::BGMode3 => {
                self.scanline_mode3(2, sysbus);
            }
            BGMode::BGMode4 => {
                self.scanline_mode4(2, &dispcnt, sysbus);
            }
            _ => panic!("{:?} not supported", dispcnt.bg_mode),
        }
    }

    pub fn render(&self) -> Vec<u32> {
        let mut buffer = vec![0u32; Gpu::DISPLAY_WIDTH * Gpu::DISPLAY_WIDTH];
        for y in 0..Gpu::DISPLAY_HEIGHT {
            for x in 0..Gpu::DISPLAY_WIDTH {
                let index = (x as usize) + (y as usize) * (512 as usize);
                let (r, g, b) = self.pixeldata[index].get_rgb24();
                buffer[x + Gpu::DISPLAY_WIDTH * y] =
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            }
        }
        buffer
    }
}

// *TODO* Running the Gpu step by step causes a massive performance impact, so for now not treat it as an emulated IO device.

impl EmuIoDev for Gpu {
    fn step(&mut self, cycles: usize, sysbus: &mut SysBus) -> (usize, Option<Interrupt>) {
        self.cycles += cycles;

        sysbus
            .ioregs
            .write_reg(REG_VCOUNT, self.current_scanline as u16);
        let mut dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

        dispstat.vcount_flag = dispstat.vcount_setting as usize == self.current_scanline;
        if dispstat.vcount_irq_enable {
            panic!("VCOUNT IRQ NOT IMPL");
        }

        match self.state {
            HDraw => {
                if self.cycles > Gpu::CYCLES_HDRAW {
                    self.current_scanline += 1;
                    self.cycles -= Gpu::CYCLES_HDRAW;

                    let (new_state, irq) = if self.current_scanline < Gpu::DISPLAY_HEIGHT {
                        self.scanline(sysbus);
                        // HBlank
                        dispstat.hblank_flag = true;
                        let irq = if dispstat.hblank_irq_enable {
                            Some(Interrupt::LCD_HBlank)
                        } else {
                            None
                        };
                        (HBlank, irq)
                    } else {
                        dispstat.vblank_flag = true;
                        let irq = if dispstat.vblank_irq_enable {
                            Some(Interrupt::LCD_VBlank)
                        } else {
                            None
                        };
                        (VBlank, irq)
                    };
                    self.state = new_state;
                    self.update_regs(dispstat, sysbus);
                    return (0, irq);
                }
            }
            HBlank => {
                if self.cycles > Gpu::CYCLES_HBLANK {
                    self.cycles -= Gpu::CYCLES_HBLANK;
                    self.state = HDraw;
                    dispstat.hblank_flag = false;
                    self.update_regs(dispstat, sysbus);
                    return (0, None);
                }
            }
            VBlank => {
                if self.cycles > Gpu::CYCLES_VBLANK {
                    self.cycles -= Gpu::CYCLES_VBLANK;
                    self.state = HDraw;
                    dispstat.vblank_flag = false;
                    self.current_scanline = 0;
                    self.scanline(sysbus);
                    self.update_regs(dispstat, sysbus);
                    return (0, None);
                }
            }
        }

        // let mut dispcnt = DisplayControl::from(sysbus.ioregs.read_reg(REG_DISPCNT));
        // let mut dispstat = DisplayStatus::from(sysbus.ioregs.read_reg(REG_DISPSTAT));

        // TODO
        (0, None)
    }
}

#[derive(Debug)]
struct TileMapEntry {
    tile_index: u32,
    x_flip: bool,
    y_flip: bool,
    palette_bank: usize,
}

impl From<u16> for TileMapEntry {
    fn from(t: u16) -> TileMapEntry {
        TileMapEntry {
            tile_index: t.bit_range(0..10) as u32,
            x_flip: t.bit(10),
            y_flip: t.bit(11),
            palette_bank: t.bit_range(12..16) as usize,
        }
    }
}
