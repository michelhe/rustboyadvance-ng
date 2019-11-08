use std::fmt;

use super::arm7tdmi::{Addr, Bus};
use super::*;

use crate::bitfield::Bit;
use crate::num::FromPrimitive;

mod mosaic;
mod obj;
mod sfx;

pub mod regs;
pub use regs::*;

pub const VRAM_ADDR: Addr = 0x0600_0000;
pub const DISPLAY_WIDTH: usize = 240;
pub const DISPLAY_HEIGHT: usize = 160;

const CYCLES_PIXEL: usize = 4;
const CYCLES_HDRAW: usize = 960;
const CYCLES_HBLANK: usize = 272;
const CYCLES_SCANLINE: usize = 1232;
const CYCLES_VDRAW: usize = 197120;
const CYCLES_VBLANK: usize = 83776;

pub const TILE_SIZE: u32 = 0x20;

// TODO - remove the one in palette.rs
bitfield! {
    #[derive(Copy, Clone, Default, PartialEq)]
    pub struct Rgb15(u16);
    impl Debug;
    pub r, set_r: 4, 0;
    pub g, set_g: 9, 5;
    pub b, set_b: 14, 10;
}

impl Rgb15 {
    pub const BLACK: Rgb15 = Rgb15(0);
    pub const WHITE: Rgb15 = Rgb15(0x7fff);
    pub const TRANSPARENT: Rgb15 = Rgb15(0x8000);

    pub fn to_rgb24(&self) -> u32 {
        ((self.r() as u32) << 19) | ((self.g() as u32) << 11) | ((self.b() as u32) << 3)
    }

    pub fn from_rgb(r: u16, g: u16, b: u16) -> Rgb15 {
        let mut c = Rgb15(0);
        c.set_r(r);
        c.set_g(g);
        c.set_b(b);
        c
    }

    pub fn get_rgb(&self) -> (u16, u16, u16) {
        (self.r(), self.g(), self.b())
    }

    pub fn is_transparent(&self) -> bool {
        self.0 == 0x8000
    }
}

#[derive(Debug, Primitive, Copy, Clone)]
pub enum PixelFormat {
    BPP4 = 0,
    BPP8 = 1,
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

pub struct FrameBuffer([u32; DISPLAY_WIDTH * DISPLAY_HEIGHT]);

impl fmt::Debug for FrameBuffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FrameBuffer: ")?;
        for i in 0..6 {
            write!(f, "#{:06x}, ", self[i])?;
        }
        write!(f, "...")
    }
}

impl std::ops::Index<usize> for FrameBuffer {
    type Output = u32;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl std::ops::IndexMut<usize> for FrameBuffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

#[derive(Copy, Clone)]
pub struct Scanline<T>([T; DISPLAY_WIDTH]);

impl Default for Scanline<Rgb15> {
    fn default() -> Scanline<Rgb15> {
        Scanline([Rgb15::TRANSPARENT; DISPLAY_WIDTH])
    }
}

impl<T> fmt::Debug for Scanline<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "...")
    }
}

impl<T> std::ops::Index<usize> for Scanline<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<T> std::ops::IndexMut<usize> for Scanline<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct Bg {
    pub bgcnt: BgControl,
    pub bgvofs: u16,
    pub bghofs: u16,
    line: Scanline<Rgb15>,

    // for mosaic
    mosaic_first_row: Scanline<Rgb15>,
}

#[derive(Debug, Default)]
pub struct Window {
    pub left: u8,
    pub right: u8,
    pub top: u8,
    pub bottom: u8,
    pub flags: WindowFlags,
}

impl Window {
    pub fn inside(&self, x: usize, y: usize) -> bool {
        let left = self.left as usize;
        let mut right = self.right as usize;
        let top = self.top as usize;
        let mut bottom = self.bottom as usize;

        if right > DISPLAY_WIDTH || right < left {
            right = DISPLAY_WIDTH;
        }
        if bottom > DISPLAY_HEIGHT || bottom < top {
            bottom = DISPLAY_HEIGHT;
        }

        (x >= left && x < right) && (y >= top && y < bottom)
    }
}

#[derive(Debug)]
pub enum WindowType {
    Win0,
    Win1,
    WinObj,
    WinOut,
    WinNone,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct BgAffine {
    pub pa: i16, // dx
    pub pb: i16, // dmx
    pub pc: i16, // dy
    pub pd: i16, // dmy
    pub x: i32,
    pub y: i32,
}

#[derive(Debug)]
pub struct Gpu {
    pub state: GpuState,
    cycles: usize,

    // registers
    pub current_scanline: usize, // VCOUNT
    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,

    pub bg: [Bg; 4],
    pub bg_aff: [BgAffine; 2],

    pub win0: Window,
    pub win1: Window,
    pub winout_flags: WindowFlags,
    pub winobj_flags: WindowFlags,

    pub mosaic: RegMosaic,
    pub bldcnt: BlendControl,
    pub bldalpha: BlendAlpha,
    pub bldy: u16,

    pub obj_line: Scanline<Rgb15>,
    pub obj_line_priorities: Scanline<u16>,
    pub frame_buffer: FrameBuffer,
}

impl Gpu {
    pub fn new() -> Gpu {
        Gpu {
            dispcnt: DisplayControl(0x80),
            dispstat: DisplayStatus(0),
            bg: [Bg::default(); 4],
            bg_aff: [BgAffine::default(); 2],
            win0: Window::default(),
            win1: Window::default(),
            winout_flags: WindowFlags::from(0),
            winobj_flags: WindowFlags::from(0),
            mosaic: RegMosaic(0),
            bldcnt: BlendControl(0),
            bldalpha: BlendAlpha(0),
            bldy: 0,

            state: HDraw,
            current_scanline: 0,
            cycles: 0,
            obj_line: Scanline::default(),
            obj_line_priorities: Scanline([3; DISPLAY_WIDTH]),
            frame_buffer: FrameBuffer([0; DISPLAY_WIDTH * DISPLAY_HEIGHT]),
        }
    }

    /// helper method that reads the palette index from a base address and x + y
    pub fn read_pixel_index(
        &self,
        sb: &SysBus,
        addr: Addr,
        x: u32,
        y: u32,
        format: PixelFormat,
    ) -> usize {
        let ofs = addr - VRAM_ADDR;
        match format {
            PixelFormat::BPP4 => {
                let byte = sb.vram.read_8(ofs + index2d!(Addr, x / 2, y, 4));
                if x & 1 != 0 {
                    (byte >> 4) as usize
                } else {
                    (byte & 0xf) as usize
                }
            }
            PixelFormat::BPP8 => sb.vram.read_8(ofs + index2d!(Addr, x, y, 8)) as usize,
        }
    }

    pub fn get_palette_color(
        &self,
        sb: &SysBus,
        index: u32,
        palette_index: u32,
        offset: u32,
    ) -> Rgb15 {
        if index == 0 || (palette_index != 0 && index % 16 == 0) {
            return Rgb15::TRANSPARENT;
        }
        Rgb15(
            sb.palette_ram
                .read_16(offset + 2 * index + 0x20 * palette_index),
        )
    }

    fn render_pixel(&mut self, x: i32, y: i32, p: Rgb15) {
        self.frame_buffer.0[index2d!(usize, x, y, DISPLAY_WIDTH)] = p.to_rgb24();
    }

    fn scanline_reg_bg(&mut self, bg: usize, sb: &mut SysBus) {
        let (h_ofs, v_ofs) = (self.bg[bg].bghofs as u32, self.bg[bg].bgvofs as u32);
        let tileset_base = self.bg[bg].bgcnt.char_block();
        let tilemap_base = self.bg[bg].bgcnt.screen_block();
        let (tile_size, pixel_format) = self.bg[bg].bgcnt.tile_format();

        let (bg_width, bg_height) = self.bg[bg].bgcnt.size_regular();

        let screen_y = self.current_scanline as u32;
        let mut screen_x = 0;

        // calculate the bg coords at the top-left corner, including wraparound
        let bg_x = (screen_x + h_ofs) % bg_width;
        let bg_y = (screen_y + v_ofs) % bg_height;

        // calculate the initial screen entry index
        // | (256,256) | (512,256) |  (256,512)  | (512,512) |
        // |-----------|-----------|-------------|-----------|
        // |           |           |     [1]     |  [2][3]   |
        // |    [0]    |  [0][1]   |     [0]     |  [0][1]   |
        // |___________|___________|_____________|___________|
        //
        let mut sbb = match (bg_width, bg_height) {
            (256, 256) => 0,
            (512, 256) => bg_x / 256,
            (256, 512) => bg_y / 256,
            (512, 512) => index2d!(u32, bg_x / 256, bg_y / 256, 2),
            _ => unreachable!(),
        } as u32;

        let mut se_row = (bg_x / 8) % 32;
        let se_column = (bg_y / 8) % 32;

        // this will be non-zero if the h-scroll lands in a middle of a tile
        let mut start_tile_x = bg_x % 8;
        let tile_py = (bg_y % 8) as u32;

        loop {
            let mut map_addr =
                tilemap_base + SCREEN_BLOCK_SIZE * sbb + 2 * index2d!(u32, se_row, se_column, 32);
            for _ in se_row..32 {
                let entry = TileMapEntry(sb.vram.read_16(map_addr - VRAM_ADDR));
                let tile_addr = tileset_base + entry.tile_index() * tile_size;

                for tile_px in start_tile_x..8 {
                    let index = self.read_pixel_index(
                        sb,
                        tile_addr,
                        if entry.x_flip() { 7 - tile_px } else { tile_px },
                        if entry.y_flip() { 7 - tile_py } else { tile_py },
                        pixel_format,
                    );
                    let palette_bank = match pixel_format {
                        PixelFormat::BPP4 => entry.palette_bank() as u32,
                        PixelFormat::BPP8 => 0u32,
                    };
                    let color = self.get_palette_color(sb, index as u32, palette_bank, 0);
                    self.bg[bg].line[screen_x as usize] = color;
                    screen_x += 1;
                    if (DISPLAY_WIDTH as u32) == screen_x {
                        return;
                    }
                }
                start_tile_x = 0;
                map_addr += 2;
            }
            se_row = 0;
            if bg_width == 512 {
                sbb = sbb ^ 1;
            }
        }
    }

    fn scanline_aff_bg(&mut self, bg: usize, sb: &mut SysBus) {
      // TODO
    }

    fn scanline_mode3(&mut self, bg: usize, sb: &mut SysBus) {
        let y = self.current_scanline;

        for x in 0..DISPLAY_WIDTH {
            let pixel_index = index2d!(u32, x, y, DISPLAY_WIDTH);
            let pixel_ofs = 2 * pixel_index;
            let color = Rgb15(sb.vram.read_16(pixel_ofs));
            self.bg[bg].line[x] = color;
        }
    }

    fn scanline_mode4(&mut self, bg: usize, sb: &mut SysBus) {
        let page_ofs: u32 = match self.dispcnt.display_frame() {
            0 => 0x0600_0000 - VRAM_ADDR,
            1 => 0x0600_a000 - VRAM_ADDR,
            _ => unreachable!(),
        };

        let y = self.current_scanline;

        for x in 0..DISPLAY_WIDTH {
            let bitmap_index = index2d!(x, y, DISPLAY_WIDTH);
            let bitmap_ofs = page_ofs + (bitmap_index as u32);
            let index = sb.vram.read_8(bitmap_ofs as Addr) as u32;
            let color = self.get_palette_color(sb, index, 0, 0);
            self.bg[bg].line[x] = color;
        }
    }

    pub fn render_scanline(&mut self, sb: &mut SysBus) {
        // TODO - also render objs
        match self.dispcnt.mode() {
            0 => {
                for bg in 0..3 {
                    if self.dispcnt.disp_bg(bg) {
                        self.scanline_reg_bg(bg, sb);
                    }
                }
            }
            1 => {
                if self.dispcnt.disp_bg(2) {
                    self.scanline_aff_bg(2, sb);
                }
                if self.dispcnt.disp_bg(1) {
                    self.scanline_reg_bg(1, sb);
                }
                if self.dispcnt.disp_bg(0) {
                    self.scanline_reg_bg(0, sb);
                }
            }
            2 => {
                if self.dispcnt.disp_bg(3) {
                    self.scanline_aff_bg(3, sb);
                }
                if self.dispcnt.disp_bg(2) {
                    self.scanline_aff_bg(2, sb);
                }
            }
            3 => {
                self.scanline_mode3(2, sb);
            }
            4 => {
                self.scanline_mode4(2, sb);
            }
            _ => panic!("{:?} not supported", self.dispcnt.mode()),
        }
        if self.dispcnt.disp_obj() {
            self.render_objs(sb);
        }
        self.mosaic_sfx();
        let post_sfx_line = self.composite_sfx(sb);
        for x in 0..DISPLAY_WIDTH {
            self.frame_buffer.0[x + self.current_scanline * DISPLAY_WIDTH] =
                post_sfx_line[x].to_rgb24();
        }
    }

    pub fn get_framebuffer(&self) -> &[u32] {
        &self.frame_buffer.0
    }
}

impl SyncedIoDevice for Gpu {
    fn step(&mut self, cycles: usize, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        self.cycles += cycles;

        if self.dispstat.vcount_setting() != 0 {
            self.dispstat
                .set_vcount(self.dispstat.vcount_setting() == self.current_scanline as u16);
        }
        if self.dispstat.vcount_irq_enable() && self.dispstat.get_vcount() {
            irqs.set_LCD_VCounterMatch(true);
        }

        match self.state {
            HDraw => {
                if self.cycles > CYCLES_HDRAW {
                    self.current_scanline += 1;
                    self.cycles -= CYCLES_HDRAW;

                    if self.current_scanline < DISPLAY_HEIGHT {
                        self.render_scanline(sb);
                        // HBlank
                        self.dispstat.set_hblank(true);
                        if self.dispstat.hblank_irq_enable() {
                            irqs.set_LCD_HBlank(true);
                        };
                        self.state = HBlank;
                    } else {
                        self.dispstat.set_vblank(true);
                        if self.dispstat.vblank_irq_enable() {
                            irqs.set_LCD_VBlank(true);
                        };
                        self.state = VBlank;
                    };
                }
            }
            HBlank => {
                if self.cycles > CYCLES_HBLANK {
                    self.cycles -= CYCLES_HBLANK;
                    self.state = HDraw;
                    self.dispstat.set_hblank(false);
                    self.dispstat.set_vblank(false);
                }
            }
            VBlank => {
                if self.cycles > CYCLES_VBLANK {
                    self.cycles -= CYCLES_VBLANK;
                    self.state = HDraw;
                    self.dispstat.set_hblank(false);
                    self.dispstat.set_vblank(false);
                    self.current_scanline = 0;
                    self.render_scanline(sb);
                }
            }
        }
    }
}

bitfield! {
    struct TileMapEntry(u16);
    u16;
    u32, tile_index, _: 9, 0;
    x_flip, _ : 10;
    y_flip, _ : 11;
    palette_bank, _ : 15, 12;
}
