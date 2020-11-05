use super::layer::RenderLayer;
use super::sfx::BldMode;
use super::*;

use num::ToPrimitive;
use serde::{Deserialize, Serialize};

pub const SCREEN_BLOCK_SIZE: u32 = 0x800;

#[derive(Debug, PartialEq)]
pub enum ObjMapping {
    TwoDimension,
    OneDimension,
}

impl DisplayControl {
    pub fn enable_bg(&self, bg: usize) -> bool {
        self.0.bit(8 + bg)
    }
    pub fn is_using_windows(&self) -> bool {
        self.enable_window0() || self.enable_window1() || self.enable_obj_window()
    }
    pub fn obj_mapping(&self) -> ObjMapping {
        if self.obj_character_vram_mapping() {
            ObjMapping::OneDimension
        } else {
            ObjMapping::TwoDimension
        }
    }
}

impl BgControl {
    pub fn char_block(&self) -> u32 {
        (self.character_base_block() as u32) * 0x4000
    }

    pub fn screen_block(&self) -> u32 {
        (self.screen_base_block() as u32) * SCREEN_BLOCK_SIZE
    }

    pub fn size_regular(&self) -> (u32, u32) {
        match self.bg_size() {
            0b00 => (256, 256),
            0b01 => (512, 256),
            0b10 => (256, 512),
            0b11 => (512, 512),
            _ => unreachable!(),
        }
    }

    pub fn size_affine(&self) -> (i32, i32) {
        let x = 128 << self.bg_size();
        (x, x)
    }

    pub fn tile_format(&self) -> (u32, PixelFormat) {
        if self.palette256() {
            (2 * TILE_SIZE, PixelFormat::BPP8)
        } else {
            (TILE_SIZE, PixelFormat::BPP4)
        }
    }
}

// struct definitions below because the bitfield! macro messes up syntax highlighting in vscode.
bitfield! {
    #[derive(Serialize, Deserialize, Clone)]
    pub struct DisplayControl(u16);
    impl Debug;
    u16;
    pub mode, set_mode: 2, 0;
    pub display_frame, set_display_frame: 4, 4;
    pub hblank_interval_free, _: 5;
    pub obj_character_vram_mapping, _: 6;
    pub force_blank, _: 7;
    pub enable_bg0, _ : 8;
    pub enable_bg1, _ : 9;
    pub enable_bg2, _ : 10;
    pub enable_bg3, _ : 11;
    pub enable_obj, _ : 12;
    pub enable_window0, _ : 13;
    pub enable_window1, _ : 14;
    pub enable_obj_window, _ : 15;
}

bitfield! {
    #[derive(Serialize, Deserialize, Clone)]
    pub struct DisplayStatus(u16);
    impl Debug;
    u16;
    pub get_vblank_flag, set_vblank_flag: 0;
    pub get_hblank_flag, set_hblank_flag: 1;
    pub get_vcount_flag, set_vcount_flag: 2;
    pub vblank_irq_enable, set_vblank_irq_enable : 3;
    pub hblank_irq_enable, set_hblank_irq_enable : 4;
    pub vcount_irq_enable, set_vcount_irq_enable : 5;
    pub vcount_setting, set_vcount_setting : 15, 8;
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone)]
    pub struct BgControl(u16);
    impl Debug;
    u16;
    pub priority, _: 1, 0;
    pub character_base_block, _: 3, 2;
    pub mosaic, _ : 6;
    pub palette256, _ : 7;
    pub screen_base_block, _: 12, 8;
    pub affine_wraparound, _: 13;
    pub bg_size, _ : 15, 14;
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
    pub struct BlendFlags: u32 {
        const BG0 = 0b00000001;
        const BG1 = 0b00000010;
        const BG2 = 0b00000100;
        const BG3 = 0b00001000;
        const OBJ = 0b00010000;
        const BACKDROP  = 0b00100000; // BACKDROP
    }
}

impl From<u16> for BlendFlags {
    fn from(v: u16) -> BlendFlags {
        BlendFlags::from_bits(v as u32).unwrap()
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

    pub fn contains_render_layer(&self, layer: &RenderLayer) -> bool {
        let layer_flags = BlendFlags::from_bits(layer.kind.to_u32().unwrap()).unwrap();
        self.contains(layer_flags)
    }
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone)]
    pub struct BlendControl(u16);
    impl Debug;
    pub into BlendFlags, top, _: 5, 0;
    pub into BldMode, mode, set_mode: 7, 6;
    pub into BlendFlags, bottom, _: 13, 8;
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone)]
    pub struct BlendAlpha(u16);
    impl Debug;
    u16;
    pub eva, _: 5, 0;
    pub evb, _: 12, 8;
}

bitflags! {
    #[derive(Serialize, Deserialize, Default)]
    pub struct WindowFlags: u32 {
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
        WindowFlags::from_bits(v as u32).unwrap()
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

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone)]
    pub struct WindowReg(u16);
    impl Debug;
    u16;
    pub into WindowFlags, lower, _: 5, 0;
    pub into WindowFlags, upper, _: 13, 8;
}
