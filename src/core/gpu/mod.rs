use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use super::super::VideoInterface;
use super::interrupt::IrqBitmask;
use super::sysbus::{BoxedMemory, SysBus};
use super::Bus;

use crate::bitfield::Bit;
use crate::num::FromPrimitive;

mod render;

use render::Point;

mod layer;
mod mosaic;
mod rgb15;
mod sfx;
mod window;

pub use rgb15::Rgb15;
pub use window::*;

pub mod regs;
pub use regs::*;

#[allow(unused)]
pub mod consts {
    pub const VIDEO_RAM_SIZE: usize = 128 * 1024;
    pub const PALETTE_RAM_SIZE: usize = 1 * 1024;
    pub const OAM_SIZE: usize = 1 * 1024;

    pub const VRAM_ADDR: u32 = 0x0600_0000;
    pub const DISPLAY_WIDTH: usize = 240;
    pub const DISPLAY_HEIGHT: usize = 160;
    pub const VBLANK_LINES: usize = 68;

    pub(super) const CYCLES_PIXEL: usize = 4;
    pub(super) const CYCLES_HDRAW: usize = 960;
    pub(super) const CYCLES_HBLANK: usize = 272;
    pub(super) const CYCLES_SCANLINE: usize = 1232;
    pub(super) const CYCLES_VDRAW: usize = 197120;
    pub(super) const CYCLES_VBLANK: usize = 83776;

    pub const TILE_SIZE: u32 = 0x20;
}
pub use self::consts::*;

#[derive(Debug, Primitive, Copy, Clone)]
pub enum PixelFormat {
    BPP4 = 0,
    BPP8 = 1,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Scanline {
    inner: Vec<Rgb15>,
}

impl Default for Scanline {
    fn default() -> Scanline {
        Scanline {
            inner: vec![Rgb15::TRANSPARENT; DISPLAY_WIDTH],
        }
    }
}

impl fmt::Debug for Scanline {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "...")
    }
}

impl std::ops::Index<usize> for Scanline {
    type Output = Rgb15;
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner[index]
    }
}

impl std::ops::IndexMut<usize> for Scanline {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.inner[index]
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Background {
    pub bgcnt: BgControl,
    pub bgvofs: u16,
    pub bghofs: u16,

    line: Scanline,

    // for mosaic
    mosaic_first_row: Scanline,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct AffineMatrix {
    pub pa: i32,
    pub pb: i32,
    pub pc: i32,
    pub pd: i32,
}

#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone)]
pub struct BgAffine {
    pub pa: i16, // dx
    pub pb: i16, // dmx
    pub pc: i16, // dy
    pub pd: i16, // dmy
    pub x: i32,
    pub y: i32,
    pub internal_x: i32,
    pub internal_y: i32,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct ObjBufferEntry {
    pub(super) window: bool,
    pub(super) alpha: bool,
    pub(super) color: Rgb15,
    pub(super) priority: u16,
}

impl Default for ObjBufferEntry {
    fn default() -> ObjBufferEntry {
        ObjBufferEntry {
            window: false,
            alpha: false,
            color: Rgb15::TRANSPARENT,
            priority: 4,
        }
    }
}

type VideoDeviceRcRefCell = Rc<RefCell<dyn VideoInterface>>;

#[derive(Serialize, Deserialize, Clone, DebugStub)]
pub struct Gpu {
    pub state: GpuState,

    /// how many cycles left until next gpu state ?
    cycles_left_for_current_state: usize,

    // registers
    pub vcount: usize, // VCOUNT
    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,

    pub bg: [Background; 4],
    pub bg_aff: [BgAffine; 2],

    pub win0: Window,
    pub win1: Window,
    pub winout_flags: WindowFlags,
    pub winobj_flags: WindowFlags,

    pub mosaic: RegMosaic,
    pub bldcnt: BlendControl,
    pub bldalpha: BlendAlpha,
    pub bldy: u16,

    pub palette_ram: BoxedMemory,
    pub vram: BoxedMemory,
    pub oam: BoxedMemory,

    #[debug_stub = "Sprite Buffer"]
    pub obj_buffer: Vec<ObjBufferEntry>,

    #[debug_stub = "Frame Buffer"]
    pub(super) frame_buffer: Vec<u32>,
}

impl Gpu {
    pub fn new() -> Gpu {
        Gpu {
            dispcnt: DisplayControl(0x80),
            dispstat: DisplayStatus(0),
            bg: [
                Background::default(),
                Background::default(),
                Background::default(),
                Background::default(),
            ],
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
            vcount: 0,
            cycles_left_for_current_state: CYCLES_HDRAW,

            palette_ram: BoxedMemory::new(vec![0; PALETTE_RAM_SIZE].into_boxed_slice()),
            vram: BoxedMemory::new(vec![0; VIDEO_RAM_SIZE].into_boxed_slice()),
            oam: BoxedMemory::new(vec![0; OAM_SIZE].into_boxed_slice()),

            obj_buffer: vec![Default::default(); DISPLAY_WIDTH * DISPLAY_HEIGHT],

            frame_buffer: vec![0; DISPLAY_WIDTH * DISPLAY_HEIGHT],
        }
    }

    pub fn skip_bios(&mut self) {
        for i in 0..2 {
            self.bg_aff[i].pa = 0x100;
            self.bg_aff[i].pb = 0;
            self.bg_aff[i].pc = 0;
            self.bg_aff[i].pd = 0x100;
        }
    }

    /// helper method that reads the palette index from a base address and x + y
    pub fn read_pixel_index(&self, addr: u32, x: u32, y: u32, format: PixelFormat) -> usize {
        let ofs = addr - VRAM_ADDR;
        match format {
            PixelFormat::BPP4 => {
                let byte = self.vram.read_8(ofs + index2d!(u32, x / 2, y, 4));
                if x & 1 != 0 {
                    (byte >> 4) as usize
                } else {
                    (byte & 0xf) as usize
                }
            }
            PixelFormat::BPP8 => self.vram.read_8(ofs + index2d!(u32, x, y, 8)) as usize,
        }
    }

    pub fn get_palette_color(&self, index: u32, palette_index: u32, offset: u32) -> Rgb15 {
        if index == 0 || (palette_index != 0 && index % 16 == 0) {
            return Rgb15::TRANSPARENT;
        }
        Rgb15(
            self.palette_ram
                .read_16(offset + 2 * index + 0x20 * palette_index),
        )
    }

    #[inline]
    pub(super) fn obj_buffer_get(&self, x: usize, y: usize) -> &ObjBufferEntry {
        &self.obj_buffer[index2d!(x, y, DISPLAY_WIDTH)]
    }

    #[inline]
    pub(super) fn obj_buffer_get_mut(&mut self, x: usize, y: usize) -> &mut ObjBufferEntry {
        &mut self.obj_buffer[index2d!(x, y, DISPLAY_WIDTH)]
    }

    pub fn get_ref_point(&self, bg: usize) -> Point {
        assert!(bg == 2 || bg == 3);
        (
            self.bg_aff[bg - 2].internal_x,
            self.bg_aff[bg - 2].internal_y,
        )
    }

    pub fn render_scanline(&mut self) {
        if self.dispcnt.enable_obj() {
            self.render_objs();
        }
        match self.dispcnt.mode() {
            0 => {
                for bg in 0..=3 {
                    if self.dispcnt.enable_bg(bg) {
                        self.render_reg_bg(bg);
                    }
                }
                self.finalize_scanline(0, 3);
            }
            1 => {
                if self.dispcnt.enable_bg(2) {
                    self.render_aff_bg(2);
                }
                if self.dispcnt.enable_bg(1) {
                    self.render_reg_bg(1);
                }
                if self.dispcnt.enable_bg(0) {
                    self.render_reg_bg(0);
                }
                self.finalize_scanline(0, 2);
            }
            2 => {
                if self.dispcnt.enable_bg(3) {
                    self.render_aff_bg(3);
                }
                if self.dispcnt.enable_bg(2) {
                    self.render_aff_bg(2);
                }
                self.finalize_scanline(2, 3);
            }
            3 => {
                self.render_mode3(2);
                self.finalize_scanline(2, 2);
            }
            4 => {
                self.render_mode4(2);
                self.finalize_scanline(2, 2);
            }
            _ => panic!("{:?} not supported", self.dispcnt.mode()),
        }
        // self.mosaic_sfx();
    }

    fn update_vcount(&mut self, value: usize, irqs: &mut IrqBitmask) {
        self.vcount = value;
        let vcount_setting = self.dispstat.vcount_setting();
        self.dispstat
            .set_vcount_flag(vcount_setting == self.vcount as u16);

        if self.dispstat.vcount_irq_enable() && self.dispstat.get_vcount_flag() {
            irqs.set_LCD_VCounterMatch(true);
        }
    }

    /// Clears the gpu obj buffer
    pub fn obj_buffer_reset(&mut self) {
        for x in self.obj_buffer.iter_mut() {
            *x = Default::default();
        }
    }

    pub fn on_state_completed(
        &mut self,
        completed: GpuState,
        sb: &mut SysBus,
        irqs: &mut IrqBitmask,
        video_device: &VideoDeviceRcRefCell,
    ) {
        match completed {
            HDraw => {
                // Transition to HBlank
                self.state = HBlank;
                self.cycles_left_for_current_state = CYCLES_HBLANK;
                self.dispstat.set_hblank_flag(true);

                if self.dispstat.hblank_irq_enable() {
                    irqs.set_LCD_HBlank(true);
                };
                sb.io.dmac.notify_hblank();
            }
            HBlank => {
                self.update_vcount(self.vcount + 1, irqs);

                if self.vcount < DISPLAY_HEIGHT {
                    self.state = HDraw;
                    self.dispstat.set_hblank_flag(false);
                    self.render_scanline();
                    // update BG2/3 reference points on the end of a scanline
                    for i in 0..2 {
                        self.bg_aff[i].internal_x += self.bg_aff[i].pb as i16 as i32;
                        self.bg_aff[i].internal_y += self.bg_aff[i].pd as i16 as i32;
                    }
                    self.cycles_left_for_current_state = CYCLES_HDRAW;
                } else {
                    self.state = VBlank;

                    // latch BG2/3 reference points on vblank
                    for i in 0..2 {
                        self.bg_aff[i].internal_x = self.bg_aff[i].x;
                        self.bg_aff[i].internal_y = self.bg_aff[i].y;
                    }

                    self.dispstat.set_vblank_flag(true);
                    self.dispstat.set_hblank_flag(false);
                    if self.dispstat.vblank_irq_enable() {
                        irqs.set_LCD_VBlank(true);
                    };

                    sb.io.dmac.notify_vblank();
                    video_device.borrow_mut().render(&self.frame_buffer);
                    self.obj_buffer_reset();
                    self.cycles_left_for_current_state = CYCLES_SCANLINE;
                }
            }
            VBlank => {
                if self.vcount < DISPLAY_HEIGHT + VBLANK_LINES - 1 {
                    self.update_vcount(self.vcount + 1, irqs);
                    self.cycles_left_for_current_state = CYCLES_SCANLINE;
                } else {
                    self.update_vcount(0, irqs);
                    self.dispstat.set_vblank_flag(false);
                    self.render_scanline();
                    self.state = HDraw;

                    self.cycles_left_for_current_state = CYCLES_HDRAW;
                }
            }
        };
    }

    // Returns the new gpu state
    pub fn step(
        &mut self,
        cycles: usize,
        sb: &mut SysBus,
        irqs: &mut IrqBitmask,
        cycles_to_next_event: &mut usize,
        video_device: &VideoDeviceRcRefCell,
    ) {
        if self.cycles_left_for_current_state <= cycles {
            let overshoot = cycles - self.cycles_left_for_current_state;

            self.on_state_completed(self.state, sb, irqs, video_device);

            // handle the overshoot
            if overshoot < self.cycles_left_for_current_state {
                self.cycles_left_for_current_state -= overshoot;
            } else {
                panic!("OH SHIT");
            }
        } else {
            self.cycles_left_for_current_state -= cycles;
        }

        if self.cycles_left_for_current_state < *cycles_to_next_event {
            *cycles_to_next_event = self.cycles_left_for_current_state;
        }
    }
}
