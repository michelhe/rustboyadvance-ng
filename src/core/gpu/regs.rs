use super::*;

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

impl DisplayControl {
    pub fn disp_bg(&self, bg: usize) -> bool {
        self.0.bit(8 + bg)
    }
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
    #[derive(Copy, Clone)]
    pub struct BgControl(u16);
    impl Debug;
    u16;
    pub bg_priority, _: 1, 0;
    pub character_base_block, _: 3, 2;
    pub moasic, _ : 6;
    pub palette256, _ : 7;
    pub screen_base_block, _: 12, 8;
    pub affine_wraparound, _: 13;
    pub bg_size, _ : 15, 14;
}

pub const SCREEN_BLOCK_SIZE: u32 = 0x800;

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

    pub fn tile_format(&self) -> (u32, PixelFormat) {
        if self.palette256() {
            (2 * Gpu::TILE_SIZE, PixelFormat::BPP8)
        } else {
            (Gpu::TILE_SIZE, PixelFormat::BPP4)
        }
    }
}
