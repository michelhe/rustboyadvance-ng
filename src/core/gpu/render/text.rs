//! Rendering for modes 0-3

use super::super::consts::*;
use super::super::{Gpu, PixelFormat, Scanline, SCREEN_BLOCK_SIZE};

use crate::core::Bus;

impl Gpu {
    pub(in super::super) fn render_reg_bg(&mut self, bg: usize) {
        let (h_ofs, v_ofs) = (self.bg[bg].bghofs as u32, self.bg[bg].bgvofs as u32);
        let tileset_base = self.bg[bg].bgcnt.char_block();
        let tilemap_base = self.bg[bg].bgcnt.screen_block();
        let (tile_size, pixel_format) = self.bg[bg].bgcnt.tile_format();

        let (bg_width, bg_height) = self.bg[bg].bgcnt.size_regular();

        let screen_y = self.vcount as u32;
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
                let entry = TileMapEntry(self.vram.read_16(map_addr - VRAM_ADDR));
                let tile_addr = tileset_base + entry.tile_index() * tile_size;

                for tile_px in start_tile_x..8 {
                    let index = self.read_pixel_index(
                        tile_addr,
                        if entry.x_flip() { 7 - tile_px } else { tile_px },
                        if entry.y_flip() { 7 - tile_py } else { tile_py },
                        pixel_format,
                    );
                    let palette_bank = match pixel_format {
                        PixelFormat::BPP4 => entry.palette_bank() as u32,
                        PixelFormat::BPP8 => 0u32,
                    };
                    let color = self.get_palette_color(index as u32, palette_bank, 0);
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

    pub(in super::super) fn render_aff_bg(&mut self, bg: usize) {
        // TODO
        self.bg[bg].line = Scanline::default();
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
