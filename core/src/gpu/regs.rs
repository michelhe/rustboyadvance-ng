use super::layer::RenderLayer;
use super::*;

use serde::{Deserialize, Serialize};

pub const SCREEN_BLOCK_SIZE: u32 = 0x800;

pub trait GpuMemoryMappedIO {
    fn read(&self) -> u16;
    fn write(&mut self, value: u16);
}

#[derive(Debug, PartialEq)]
pub enum ObjMapping {
    TwoDimension,
    OneDimension,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DisplayControl {
    pub mode: u16,
    pub display_frame_select: u16,
    pub hblank_interval_free: bool,
    pub obj_character_vram_mapping: bool,
    pub force_blank: bool,
    pub enable_bg: [bool; 4],
    pub enable_obj: bool,
    pub enable_window0: bool,
    pub enable_window1: bool,
    pub enable_obj_window: bool,
}

impl From<u16> for DisplayControl {
    fn from(value: u16) -> DisplayControl {
        let mut dispcnt = DisplayControl::default();
        dispcnt.write(value);
        dispcnt
    }
}

impl DisplayControl {
    #[inline]
    pub fn is_using_windows(&self) -> bool {
        self.enable_window0 || self.enable_window1 || self.enable_obj_window
    }
    #[inline]
    pub fn obj_mapping(&self) -> ObjMapping {
        if self.obj_character_vram_mapping {
            ObjMapping::OneDimension
        } else {
            ObjMapping::TwoDimension
        }
    }
}

impl GpuMemoryMappedIO for DisplayControl {
    #[inline]
    fn write(&mut self, value: u16) {
        self.mode = value & 0b111;
        self.display_frame_select = (value >> 4) & 1;
        self.hblank_interval_free = (value >> 5) & 1 != 0;
        self.obj_character_vram_mapping = (value >> 6) & 1 != 0;
        self.force_blank = (value >> 7) & 1 != 0;
        self.enable_bg[0] = (value >> 8) & 1 != 0;
        self.enable_bg[1] = (value >> 9) & 1 != 0;
        self.enable_bg[2] = (value >> 10) & 1 != 0;
        self.enable_bg[3] = (value >> 11) & 1 != 0;
        self.enable_obj = (value >> 12) & 1 != 0;
        self.enable_window0 = (value >> 13) & 1 != 0;
        self.enable_window1 = (value >> 14) & 1 != 0;
        self.enable_obj_window = (value >> 15) & 1 != 0;
    }
    #[inline]
    fn read(&self) -> u16 {
        self.mode
            | self.display_frame_select << 4
            | u16::from(self.hblank_interval_free) << 5
            | u16::from(self.obj_character_vram_mapping) << 6
            | u16::from(self.force_blank) << 7
            | u16::from(self.enable_bg[0]) << 8
            | u16::from(self.enable_bg[1]) << 9
            | u16::from(self.enable_bg[2]) << 10
            | u16::from(self.enable_bg[3]) << 11
            | u16::from(self.enable_obj) << 12
            | u16::from(self.enable_window0) << 13
            | u16::from(self.enable_window1) << 14
            | u16::from(self.enable_obj_window) << 15
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DisplayStatus {
    pub vblank_flag: bool,
    pub hblank_flag: bool,
    pub vcount_flag: bool,
    pub vblank_irq_enable: bool,
    pub hblank_irq_enable: bool,
    pub vcount_irq_enable: bool,
    pub vcount_setting: usize,
}

impl GpuMemoryMappedIO for DisplayStatus {
    #[inline]
    fn write(&mut self, value: u16) {
        // self.vblank_flag = (value >> 0) & 1 != 0;
        // self.hblank_flag = (value >> 1) & 1 != 0;
        // self.vcount_flag = (value >> 2) & 1 != 0;
        self.vblank_irq_enable = (value >> 3) & 1 != 0;
        self.hblank_irq_enable = (value >> 4) & 1 != 0;
        self.vcount_irq_enable = (value >> 5) & 1 != 0;
        self.vcount_setting = usize::from((value >> 8) & 0xff);
    }
    #[inline]
    fn read(&self) -> u16 {
        u16::from(self.vblank_flag) << 0
            | u16::from(self.hblank_flag) << 1
            | u16::from(self.vcount_flag) << 2
            | u16::from(self.vblank_irq_enable) << 3
            | u16::from(self.hblank_irq_enable) << 4
            | u16::from(self.vcount_irq_enable) << 5
            | (self.vcount_setting as u16) << 8
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BgControl {
    pub priority: u16,
    pub character_base_block: u16,
    pub screen_base_block: u16,
    pub mosaic: bool,
    pub palette256: bool,
    pub affine_wraparound: bool,
    pub size: u8,
}

impl GpuMemoryMappedIO for BgControl {
    #[inline]
    fn write(&mut self, value: u16) {
        self.priority = (value >> 0) & 0b11;
        self.character_base_block = (value >> 2) & 0b11;
        self.mosaic = (value >> 6) & 1 != 0;
        self.palette256 = (value >> 7) & 1 != 0;
        self.screen_base_block = (value >> 8) & 0b11111;
        self.affine_wraparound = (value >> 13) & 1 != 0;
        self.size = ((value >> 14) & 0b11) as u8;
    }

    #[inline]
    fn read(&self) -> u16 {
        self.priority
            | self.character_base_block << 2
            | u16::from(self.mosaic) << 6
            | u16::from(self.palette256) << 7
            | self.screen_base_block << 8
            | u16::from(self.affine_wraparound) << 13
            | u16::from(self.size) << 14
    }
}

impl BgControl {
    #[inline]
    pub fn char_block(&self) -> u32 {
        (self.character_base_block as u32) * 0x4000
    }
    #[inline]
    pub fn screen_block(&self) -> u32 {
        (self.screen_base_block as u32) * SCREEN_BLOCK_SIZE
    }
    #[inline]
    pub fn size_regular(&self) -> (u32, u32) {
        match self.size {
            0b00 => (256, 256),
            0b01 => (512, 256),
            0b10 => (256, 512),
            0b11 => (512, 512),
            _ => unreachable!(),
        }
    }
    #[inline]
    pub fn tile_format(&self) -> (u32, PixelFormat) {
        if self.palette256 {
            (2 * TILE_SIZE, PixelFormat::BPP8)
        } else {
            (TILE_SIZE, PixelFormat::BPP4)
        }
    }
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone)]
    pub struct RegMosaic(u16);
    impl Debug;
    u32;
    pub bg_hsize, _: 3, 0;
    pub bg_vsize, _: 7, 4;
    pub obj_hsize, _ : 11, 8;
    pub obj_vsize, _ : 15, 12;
}

bitflags! {
    #[derive(Serialize, Deserialize, Default)]
    pub struct BlendFlags: u16 {
        const BG0 = 0b00000001;
        const BG1 = 0b00000010;
        const BG2 = 0b00000100;
        const BG3 = 0b00001000;
        const OBJ = 0b00010000;
        const BACKDROP  = 0b00100000; // BACKDROP
    }
}

impl BlendFlags {
    const BG_LAYER_FLAG: [BlendFlags; 4] = [
        BlendFlags::BG0,
        BlendFlags::BG1,
        BlendFlags::BG2,
        BlendFlags::BG3,
    ];
    #[inline]
    pub fn from_bg(bg: usize) -> BlendFlags {
        Self::BG_LAYER_FLAG[bg]
    }
    #[inline]
    pub fn obj_enabled(&self) -> bool {
        self.contains(BlendFlags::OBJ)
    }
    #[inline]
    pub fn contains_render_layer(&self, layer: &RenderLayer) -> bool {
        let layer_flags = BlendFlags::from_bits_truncate(layer.kind as u16);
        self.contains(layer_flags)
    }
}

#[derive(SmartDefault, Debug, Serialize, Deserialize, Primitive, PartialEq, Clone, Copy)]
pub enum BlendMode {
    #[default]
    BldNone = 0b00,
    BldAlpha = 0b01,
    BldWhite = 0b10,
    BldBlack = 0b11,
}

#[derive(Debug, Serialize, Deserialize, Default, Copy, Clone)]
pub struct BlendControl {
    pub target1: BlendFlags,
    pub target2: BlendFlags,
    pub mode: BlendMode,
}

impl GpuMemoryMappedIO for BlendControl {
    #[inline]
    fn write(&mut self, value: u16) {
        self.target1 = BlendFlags::from_bits_truncate((value >> 0) & 0x3f);
        self.target2 = BlendFlags::from_bits_truncate((value >> 8) & 0x3f);
        self.mode = BlendMode::from_u16((value >> 6) & 0b11).unwrap_or_else(|| unreachable!());
    }

    #[inline]
    fn read(&self) -> u16 {
        (self.target1.bits() << 0) | (self.mode as u16) << 6 | (self.target2.bits() << 8)
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Copy, Clone)]
pub struct BlendAlpha {
    pub eva: u16,
    pub evb: u16,
}

impl GpuMemoryMappedIO for BlendAlpha {
    #[inline]
    fn write(&mut self, value: u16) {
        self.eva = value & 0x1f;
        self.evb = (value >> 8) & 0x1f;
    }

    #[inline]
    fn read(&self) -> u16 {
        self.eva | self.evb << 8
    }
}

bitflags! {
    #[derive(Serialize, Deserialize, Default)]
    pub struct WindowFlags: u16 {
        const BG0 = 0b00000001;
        const BG1 = 0b00000010;
        const BG2 = 0b00000100;
        const BG3 = 0b00001000;
        const OBJ = 0b00010000;
        const SFX = 0b00100000;
    }
}

impl From<u16> for WindowFlags {
    fn from(v: u16) -> WindowFlags {
        WindowFlags::from_bits_truncate(v)
    }
}

impl WindowFlags {
    pub fn sfx_enabled(&self) -> bool {
        self.contains(WindowFlags::SFX)
    }
    pub fn bg_enabled(&self, bg: usize) -> bool {
        self.contains(BG_WIN_FLAG[bg])
    }
    pub fn obj_enabled(&self) -> bool {
        self.contains(WindowFlags::OBJ)
    }
}

const BG_WIN_FLAG: [WindowFlags; 4] = [
    WindowFlags::BG0,
    WindowFlags::BG1,
    WindowFlags::BG2,
    WindowFlags::BG3,
];
