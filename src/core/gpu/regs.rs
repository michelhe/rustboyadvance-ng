use super::sfx::BldMode;
use super::*;

pub const SCREEN_BLOCK_SIZE: u32 = 0x800;

impl DisplayControl {
    pub fn disp_bg(&self, bg: usize) -> bool {
        self.0.bit(8 + bg)
    }
    pub fn is_using_windows(&self) -> bool {
        self.disp_window0() || self.disp_window1() || self.disp_obj_window()
    }
}

impl BgControl {
    pub fn char_block(&self) -> u32 {
        VRAM_ADDR + (self.character_base_block() as u32) * 0x4000
    }

    pub fn screen_block(&self) -> u32 {
        VRAM_ADDR + (self.screen_base_block() as u32) * SCREEN_BLOCK_SIZE
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

    pub fn size_affine(&self) -> (u32, u32) {
        match self.bg_size() {
            0b00 => (128, 128),
            0b01 => (256, 256),
            0b10 => (512, 512),
            0b11 => (1024, 1024),
            _ => unreachable!(),
        }
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
    pub struct DisplayControl(u16);
    impl Debug;
    u16;
    pub into BGMode, mode, set_mode: 2, 0;
    pub display_frame, set_display_frame: 4, 4;
    pub hblank_interval_free, _: 5;
    pub obj_character_vram_mapping, _: 6;
    pub forst_vblank, _: 7;
    pub disp_bg0, _ : 8;
    pub disp_bg1, _ : 9;
    pub disp_bg2, _ : 10;
    pub disp_bg3, _ : 11;
    pub disp_obj, _ : 12;
    pub disp_window0, _ : 13;
    pub disp_window1, _ : 14;
    pub disp_obj_window, _ : 15;
}

bitfield! {
    pub struct DisplayStatus(u16);
    impl Debug;
    u16;
    pub get_vblank, set_vblank: 0;
    pub get_hblank, set_hblank: 1;
    pub get_vcount, set_vcount: 2;
    pub vblank_irq_enable, _ : 3;
    pub hblank_irq_enable, _ : 4;
    pub vcount_irq_enable, _ : 5;
    pub vcount_setting, _ : 15, 8;
}

bitfield! {
    #[derive(Default, Copy, Clone)]
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
    #[derive(Default, Copy, Clone)]
    pub struct RegMosaic(u16);
    impl Debug;
    u32;
    pub bg_hsize, _: 3, 0;
    pub bg_vsize, _: 7, 4;
    pub obj_hsize, _ : 11, 8;
    pub obj_vsize, _ : 15, 12;
}

bitflags! {
    #[derive(Default)]
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

    pub fn from_bg(bg: usize) -> BlendFlags {
        Self::BG_LAYER_FLAG[bg]
    }
}

bitfield! {
    #[derive(Default, Copy, Clone)]
    pub struct BlendControl(u16);
    impl Debug;
    pub into BlendFlags, top, _: 5, 0;
    pub into BldMode, mode, set_mode: 7, 6;
    pub into BlendFlags, bottom, _: 13, 8;
}

bitfield! {
    #[derive(Default, Copy, Clone)]
    pub struct BlendAlpha(u16);
    impl Debug;
    u16;
    pub eva, _: 5, 0;
    pub evb, _: 12, 8;
}

bitflags! {
    #[derive(Default)]
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
}

const BG_WIN_FLAG: [WindowFlags; 4] = [
    WindowFlags::BG0,
    WindowFlags::BG1,
    WindowFlags::BG2,
    WindowFlags::BG3,
];

bitfield! {
    #[derive(Default, Copy, Clone)]
    pub struct WindowReg(u16);
    impl Debug;
    u16;
    pub into WindowFlags, lower, _: 5, 0;
    pub into WindowFlags, upper, _: 13, 8;
}
