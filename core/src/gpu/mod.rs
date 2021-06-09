#[cfg(not(feature = "no_video_interface"))]
use std::cell::RefCell;
#[cfg(not(feature = "no_video_interface"))]
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use super::bus::*;
use super::dma::{DmaNotifer, TIMING_HBLANK, TIMING_VBLANK};
use super::interrupt::{self, Interrupt, InterruptConnect, SharedInterruptFlags};
use super::sched::*;
pub use super::sysbus::consts::*;
#[cfg(not(feature = "no_video_interface"))]
use super::VideoInterface;

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

#[cfg(feature = "debugger")]
use std::fmt;

#[allow(unused)]
pub mod consts {
    pub use super::VRAM_ADDR;
    pub const VIDEO_RAM_SIZE: usize = 128 * 1024;
    pub const PALETTE_RAM_SIZE: usize = 1 * 1024;
    pub const OAM_SIZE: usize = 1 * 1024;

    pub const DISPLAY_WIDTH: usize = 240;
    pub const DISPLAY_HEIGHT: usize = 160;
    pub const VBLANK_LINES: usize = 68;

    pub(super) const CYCLES_PIXEL: usize = 4;
    pub(super) const CYCLES_HDRAW: usize = 960 + 46;
    pub(super) const CYCLES_HBLANK: usize = 272 - 46;
    pub(super) const CYCLES_SCANLINE: usize = 1232;
    pub(super) const CYCLES_VDRAW: usize = 197120;
    pub(super) const CYCLES_VBLANK: usize = 83776;

    pub const CYCLES_FULL_REFRESH: usize = 280896;

    pub const TILE_SIZE: u32 = 0x20;

    pub(super) const VRAM_OBJ_TILES_START_TEXT: u32 = 0x1_0000;
    pub(super) const VRAM_OBJ_TILES_START_BITMAP: u32 = 0x1_4000;
}
pub use self::consts::*;

#[derive(Debug, Primitive, Copy, Clone)]
pub enum PixelFormat {
    BPP4 = 0,
    BPP8 = 1,
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

#[cfg(not(feature = "no_video_interface"))]
type VideoDeviceRcRefCell = Rc<RefCell<dyn VideoInterface>>;

#[derive(Serialize, Deserialize, Clone, DebugStub)]
pub struct Gpu {
    interrupt_flags: SharedInterruptFlags,

    /// When deserializing this struct using serde, make sure to call connect_scheduler
    #[serde(skip)]
    #[serde(default = "Scheduler::new_shared")]
    scheduler: SharedScheduler,

    /// how many cycles left until next gpu state ?
    cycles_left_for_current_state: usize,

    // registers
    pub vcount: usize, // VCOUNT
    pub dispcnt: DisplayControl,
    pub dispstat: DisplayStatus,

    pub bgcnt: [BgControl; 4],
    pub bg_vofs: [u16; 4],
    pub bg_hofs: [u16; 4],
    pub bg_aff: [BgAffine; 2],
    pub win0: Window,
    pub win1: Window,
    pub winout_flags: WindowFlags,
    pub winobj_flags: WindowFlags,
    pub mosaic: RegMosaic,
    pub bldcnt: BlendControl,
    pub bldalpha: BlendAlpha,
    pub bldy: u16,
    pub palette_ram: Box<[u8]>,
    pub vram: Box<[u8]>,
    pub oam: Box<[u8]>,
    pub(super) vram_obj_tiles_start: u32,
    pub(super) obj_buffer: Box<[ObjBufferEntry]>,
    pub(super) frame_buffer: Box<[u32]>,
    pub(super) bg_line: [Box<[Rgb15]>; 4],
}

impl InterruptConnect for Gpu {
    fn connect_irq(&mut self, interrupt_flags: SharedInterruptFlags) {
        self.interrupt_flags = interrupt_flags;
    }
}

impl SchedulerConnect for Gpu {
    fn connect_scheduler(&mut self, scheduler: SharedScheduler) {
        self.scheduler = scheduler;
    }
}

impl Gpu {
    pub fn new(mut scheduler: SharedScheduler, interrupt_flags: SharedInterruptFlags) -> Gpu {
        scheduler.push_gpu_event(GpuEvent::HDraw, CYCLES_HDRAW);

        fn alloc_scanline_buffer() -> Box<[Rgb15]> {
            vec![Rgb15::TRANSPARENT; DISPLAY_WIDTH].into_boxed_slice()
        }

        Gpu {
            interrupt_flags,
            scheduler,
            dispcnt: DisplayControl::from(0x80),
            dispstat: Default::default(),
            bgcnt: Default::default(),
            bg_vofs: [0; 4],
            bg_hofs: [0; 4],
            bg_aff: [BgAffine::default(); 2],
            win0: Window::default(),
            win1: Window::default(),
            winout_flags: WindowFlags::from(0),
            winobj_flags: WindowFlags::from(0),
            mosaic: RegMosaic(0),
            bldcnt: BlendControl::default(),
            bldalpha: BlendAlpha::default(),
            bldy: 0,

            vcount: 0,
            cycles_left_for_current_state: CYCLES_HDRAW,
            palette_ram: vec![0; PALETTE_RAM_SIZE].into_boxed_slice(),
            vram: vec![0; VIDEO_RAM_SIZE].into_boxed_slice(),
            oam: vec![0; OAM_SIZE].into_boxed_slice(),
            obj_buffer: vec![Default::default(); DISPLAY_WIDTH * DISPLAY_HEIGHT].into_boxed_slice(),
            frame_buffer: vec![0; DISPLAY_WIDTH * DISPLAY_HEIGHT].into_boxed_slice(),
            bg_line: [
                alloc_scanline_buffer(),
                alloc_scanline_buffer(),
                alloc_scanline_buffer(),
                alloc_scanline_buffer(),
            ],
            vram_obj_tiles_start: VRAM_OBJ_TILES_START_TEXT,
        }
    }

    #[inline]
    pub fn write_dispcnt(&mut self, value: u16) {
        let old_mode = self.dispcnt.mode;
        self.dispcnt.write(value);
        let new_mode = self.dispcnt.mode;
        if old_mode != new_mode {
            debug!("[GPU] Display mode changed! {} -> {}", old_mode, new_mode);
            self.vram_obj_tiles_start = if new_mode >= 3 {
                VRAM_OBJ_TILES_START_BITMAP
            } else {
                VRAM_OBJ_TILES_START_TEXT
            };
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
    pub fn read_pixel_index(&mut self, addr: u32, x: u32, y: u32, format: PixelFormat) -> usize {
        match format {
            PixelFormat::BPP4 => self.read_pixel_index_bpp4(addr, x, y),
            PixelFormat::BPP8 => self.read_pixel_index_bpp8(addr, x, y),
        }
    }

    #[inline]
    pub fn read_pixel_index_bpp4(&mut self, addr: u32, x: u32, y: u32) -> usize {
        let ofs = addr + index2d!(u32, x / 2, y, 4);
        let ofs = ofs as usize;
        let byte = self.vram.read_8(ofs as u32);
        if x & 1 != 0 {
            (byte >> 4) as usize
        } else {
            (byte & 0xf) as usize
        }
    }

    #[inline]
    pub fn read_pixel_index_bpp8(&mut self, addr: u32, x: u32, y: u32) -> usize {
        let ofs = addr;
        self.vram.read_8(ofs + index2d!(u32, x, y, 8)) as usize
    }

    #[inline(always)]
    pub fn get_palette_color(&mut self, index: u32, palette_bank: u32, offset: u32) -> Rgb15 {
        if index == 0 || (palette_bank != 0 && index % 16 == 0) {
            return Rgb15::TRANSPARENT;
        }
        let value = self
            .palette_ram
            .read_16(offset + 2 * index + 0x20 * palette_bank);

        // top bit is ignored
        Rgb15(value & 0x7FFF)
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
        if self.dispcnt.force_blank {
            for x in self.frame_buffer[self.vcount * DISPLAY_WIDTH..]
                .iter_mut()
                .take(DISPLAY_WIDTH)
            {
                *x = 0xf8f8f8;
            }
            return;
        }

        if self.dispcnt.enable_obj {
            self.render_objs();
        }
        match self.dispcnt.mode {
            0 => {
                for bg in 0..=3 {
                    if self.dispcnt.enable_bg[bg] {
                        self.render_reg_bg(bg);
                    }
                }
                self.finalize_scanline(0, 3);
            }
            1 => {
                if self.dispcnt.enable_bg[2] {
                    self.render_aff_bg(2);
                }
                if self.dispcnt.enable_bg[1] {
                    self.render_reg_bg(1);
                }
                if self.dispcnt.enable_bg[0] {
                    self.render_reg_bg(0);
                }
                self.finalize_scanline(0, 2);
            }
            2 => {
                if self.dispcnt.enable_bg[3] {
                    self.render_aff_bg(3);
                }
                if self.dispcnt.enable_bg[2] {
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
            5 => {
                self.render_mode5(2);
                self.finalize_scanline(2, 2);
            }
            _ => panic!("{:?} not supported", self.dispcnt.mode),
        }
        // self.mosaic_sfx();
    }

    /// Clears the gpu obj buffer
    pub fn obj_buffer_reset(&mut self) {
        for x in self.obj_buffer.iter_mut() {
            *x = Default::default();
        }
    }

    pub fn get_frame_buffer(&self) -> &[u32] {
        &self.frame_buffer
    }

    #[inline]
    fn update_vcount(&mut self, value: usize) {
        self.vcount = value;
        let vcount_setting = self.dispstat.vcount_setting;
        self.dispstat.vcount_flag = vcount_setting == self.vcount;

        if self.dispstat.vcount_irq_enable && self.dispstat.vcount_flag {
            interrupt::signal_irq(&self.interrupt_flags, Interrupt::LCD_VCounterMatch);
        }
    }

    #[inline]
    fn handle_hdraw_end<D: DmaNotifer>(&mut self, dma_notifier: &mut D) -> (GpuEvent, usize) {
        self.dispstat.hblank_flag = true;
        if self.dispstat.hblank_irq_enable {
            interrupt::signal_irq(&self.interrupt_flags, Interrupt::LCD_HBlank);
        };
        dma_notifier.notify(TIMING_HBLANK);

        // Next event
        (GpuEvent::HBlank, CYCLES_HBLANK)
    }

    fn handle_hblank_end<D: DmaNotifer>(
        &mut self,
        dma_notifier: &mut D,
        #[cfg(not(feature = "no_video_interface"))] video_device: &VideoDeviceRcRefCell,
    ) -> (GpuEvent, usize) {
        self.update_vcount(self.vcount + 1);

        if self.vcount < DISPLAY_HEIGHT {
            self.dispstat.hblank_flag = false;
            self.render_scanline();
            // update BG2/3 reference points on the end of a scanline
            for i in 0..2 {
                self.bg_aff[i].internal_x += self.bg_aff[i].pb as i16 as i32;
                self.bg_aff[i].internal_y += self.bg_aff[i].pd as i16 as i32;
            }

            (GpuEvent::HDraw, CYCLES_HDRAW)
        } else {
            // latch BG2/3 reference points on vblank
            for i in 0..2 {
                self.bg_aff[i].internal_x = self.bg_aff[i].x;
                self.bg_aff[i].internal_y = self.bg_aff[i].y;
            }

            self.dispstat.vblank_flag = true;
            self.dispstat.hblank_flag = false;
            if self.dispstat.vblank_irq_enable {
                interrupt::signal_irq(&self.interrupt_flags, Interrupt::LCD_VBlank);
            };

            dma_notifier.notify(TIMING_VBLANK);

            #[cfg(not(feature = "no_video_interface"))]
            video_device.borrow_mut().render(&self.frame_buffer);

            self.obj_buffer_reset();

            (GpuEvent::VBlankHDraw, CYCLES_HDRAW)
        }
    }

    fn handle_vblank_hdraw_end(&mut self) -> (GpuEvent, usize) {
        self.dispstat.hblank_flag = true;
        if self.dispstat.hblank_irq_enable {
            interrupt::signal_irq(&self.interrupt_flags, Interrupt::LCD_HBlank);
        };
        (GpuEvent::VBlankHBlank, CYCLES_HBLANK)
    }

    fn handle_vblank_hblank_end(&mut self) -> (GpuEvent, usize) {
        if self.vcount < DISPLAY_HEIGHT + VBLANK_LINES - 1 {
            self.update_vcount(self.vcount + 1);
            self.dispstat.hblank_flag = false;
            (GpuEvent::VBlankHDraw, CYCLES_HDRAW)
        } else {
            self.update_vcount(0);
            self.dispstat.vblank_flag = false;
            self.dispstat.hblank_flag = false;
            self.render_scanline();
            (GpuEvent::HDraw, CYCLES_HDRAW)
        }
    }

    pub fn on_event<D>(
        &mut self,
        event: GpuEvent,
        extra_cycles: usize,
        dma_notifier: &mut D,
        #[cfg(not(feature = "no_video_interface"))] video_device: &VideoDeviceRcRefCell,
    ) where
        D: DmaNotifer,
    {
        let (next_event, cycles) = match event {
            GpuEvent::HDraw => self.handle_hdraw_end(dma_notifier),
            GpuEvent::HBlank => self.handle_hblank_end(
                dma_notifier,
                #[cfg(not(feature = "no_video_interface"))]
                video_device,
            ),
            GpuEvent::VBlankHDraw => self.handle_vblank_hdraw_end(),
            GpuEvent::VBlankHBlank => self.handle_vblank_hblank_end(),
        };
        self.scheduler
            .push(EventType::Gpu(next_event), cycles - extra_cycles);
    }
}

impl Bus for Gpu {
    fn read_8(&mut self, addr: Addr) -> u8 {
        let page = (addr >> 24) as usize;
        match page {
            PAGE_PALRAM => self.palette_ram.read_8(addr & 0x3ff),
            PAGE_VRAM => {
                // complicated
                let mut ofs = addr & ((VIDEO_RAM_SIZE as u32) - 1);
                if ofs > 0x18000 {
                    ofs -= 0x8000;
                }
                self.vram.read_8(ofs)
            }
            PAGE_OAM => self.oam.read_8(addr & 0x3ff),
            _ => unreachable!(),
        }
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        let page = (addr >> 24) as usize;
        match page {
            PAGE_PALRAM => self.palette_ram.write_16(addr & 0x3fe, value),
            PAGE_VRAM => {
                let mut ofs = addr & ((VIDEO_RAM_SIZE as u32) - 1);
                if ofs > 0x18000 {
                    ofs -= 0x8000;
                }
                self.vram.write_16(ofs, value)
            }
            PAGE_OAM => self.oam.write_16(addr & 0x3fe, value),
            _ => unreachable!(),
        }
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        fn expand_value(value: u8) -> u16 {
            (value as u16) * 0x101
        }

        let page = (addr >> 24) as usize;
        match page {
            PAGE_PALRAM => self.palette_ram.write_16(addr & 0x3fe, expand_value(value)),
            PAGE_VRAM => {
                let mut ofs = addr & ((VIDEO_RAM_SIZE as u32) - 1);
                if ofs > 0x18000 {
                    ofs -= 0x8000;
                }
                if ofs < self.vram_obj_tiles_start {
                    self.vram.write_16(ofs & !1, expand_value(value));
                }
            }
            PAGE_OAM => { /* OAM can't be written with 8bit store */ }
            _ => unreachable!(),
        };
    }
}

impl DebugRead for Gpu {
    fn debug_read_8(&mut self, addr: Addr) -> u8 {
        let page = (addr >> 24) as usize;
        match page {
            PAGE_PALRAM => self.palette_ram.read_8(addr & 0x3ff),
            PAGE_VRAM => self.vram.read_8(addr & ((VIDEO_RAM_SIZE as u32) - 1)),
            PAGE_OAM => self.oam.read_8(addr & 0x3ff),
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "debugger")]
impl fmt::Display for Gpu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ansi_term::Style;
        writeln!(f, "{}", Style::new().bold().paint("GPU Status:"))?;
        writeln!(f, "\tVCOUNT: {}", self.vcount)?;
        writeln!(f, "\tDISPCNT: {:?}", self.dispcnt)?;
        writeln!(f, "\tDISPSTAT: {:?}", self.dispstat)?;
        writeln!(f, "\tWIN0: {:?}", self.win0)?;
        writeln!(f, "\tWIN1: {:?}", self.win1)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct NopDmaNotifer;
    impl DmaNotifer for NopDmaNotifer {
        fn notify(&mut self, _timing: u16) {}
    }

    #[derive(Default)]
    struct TestVideoInterface {
        frame_counter: usize,
    }

    #[cfg(not(feature = "no_video_interface"))]
    impl VideoInterface for TestVideoInterface {
        fn render(&mut self, _buffer: &[u32]) {
            self.frame_counter += 1;
        }
    }

    #[test]
    fn test_gpu_state_machine() {
        let mut sched = Scheduler::new_shared();
        let mut gpu = Gpu::new(sched.clone(), Rc::new(Cell::new(Default::default())));
        #[cfg(not(feature = "no_video_interface"))]
        let video = Rc::new(RefCell::new(TestVideoInterface::default()));
        #[cfg(not(feature = "no_video_interface"))]
        let video_clone: VideoDeviceRcRefCell = video.clone();
        let mut dma_notifier = NopDmaNotifer;

        gpu.dispstat.vcount_setting = 0;
        gpu.dispstat.vcount_irq_enable = true;

        macro_rules! update {
            ($cycles:expr) => {
                sched.update($cycles);
                let (event, cycles_late) = sched.pop_pending_event().unwrap();
                assert_eq!(cycles_late, 0);
                match event {
                    EventType::Gpu(event) => gpu.on_event(
                        event,
                        cycles_late,
                        &mut dma_notifier,
                        #[cfg(not(feature = "no_video_interface"))]
                        &video_clone,
                    ),
                    _ => panic!("Found unexpected event in queue!"),
                }
            };
        }

        for line in 0..160 {
            println!("line = {}", line);
            #[cfg(not(feature = "no_video_interface"))]
            assert_eq!(video.borrow().frame_counter, 0);
            assert_eq!(gpu.vcount, line);
            assert_eq!(sched.peek_next(), Some(EventType::Gpu(GpuEvent::HDraw)));
            assert_eq!(gpu.dispstat.hblank_flag, false);
            assert_eq!(gpu.dispstat.vblank_flag, false);

            update!(CYCLES_HDRAW);

            println!("{:?}", sched.num_pending_events());
            assert_eq!(sched.peek_next(), Some(EventType::Gpu(GpuEvent::HBlank)));
            assert_eq!(gpu.dispstat.hblank_flag, true);
            assert_eq!(gpu.dispstat.vblank_flag, false);

            update!(CYCLES_HBLANK);

            assert_eq!(gpu.interrupt_flags.get().LCD_VCounterMatch(), false);
        }

        #[cfg(not(feature = "no_video_interface"))]
        assert_eq!(video.borrow().frame_counter, 1);

        for line in 0..68 {
            println!("line = {}", 160 + line);
            assert_eq!(gpu.dispstat.hblank_flag, false);
            assert_eq!(gpu.dispstat.vblank_flag, true);
            assert_eq!(
                sched.peek_next(),
                Some(EventType::Gpu(GpuEvent::VBlankHDraw))
            );

            update!(CYCLES_HDRAW);

            assert_eq!(gpu.dispstat.hblank_flag, true);
            assert_eq!(gpu.dispstat.vblank_flag, true);
            assert_eq!(
                sched.peek_next(),
                Some(EventType::Gpu(GpuEvent::VBlankHBlank))
            );
            assert_eq!(gpu.interrupt_flags.get().LCD_VCounterMatch(), false);

            update!(CYCLES_HBLANK);
        }

        #[cfg(not(feature = "no_video_interface"))]
        assert_eq!(video.borrow().frame_counter, 1);
        assert_eq!(sched.timestamp(), CYCLES_FULL_REFRESH);

        assert_eq!(gpu.interrupt_flags.get().LCD_VCounterMatch(), true);
        assert_eq!(gpu.cycles_left_for_current_state, CYCLES_HDRAW);
        assert_eq!(sched.peek_next(), Some(EventType::Gpu(GpuEvent::HDraw)));
        assert_eq!(gpu.vcount, 0);
        assert_eq!(gpu.dispstat.vcount_flag, true);
        assert_eq!(gpu.dispstat.hblank_flag, false);
    }
}
