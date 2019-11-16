use super::super::SysBus;
use super::regs::*;
use super::*;

use crate::core::sysbus::OAM_ADDR;

const OVRAM: u32 = 0x0601_0000;
const PALRAM_OFS_FG: u32 = 0x200;
const ATTRS_SIZE: u32 = 2 * 3 + 2;

struct ObjAttrs(Attribute0, Attribute1, Attribute2);

struct ObjAffineParams {
    pa: i16,
    pb: i16,
    pc: i16,
    pd: i16,
}

const AFFINE_FILL: u32 = 2 * 3;

impl ObjAffineParams {
    fn from_index(sb: &SysBus, index: u32) -> ObjAffineParams {
        let mut offset = AFFINE_FILL + index * 16 * 2;
        let pa = sb.read_16(offset) as i16;
        offset += 2 + AFFINE_FILL;
        let pb = sb.read_16(offset) as i16;
        offset += 2 + AFFINE_FILL;
        let pc = sb.read_16(offset) as i16;
        offset += 2 + AFFINE_FILL;
        let pd = sb.read_16(offset) as i16;

        ObjAffineParams { pa, pb, pc, pd }
    }
}

impl ObjAttrs {
    fn size(&self) -> (i32, i32) {
        match (self.1.size(), self.0.shape()) {
            (0, 0) /* Square */  => (8, 8),
            (1, 0) /* Square */  => (16, 16),
            (2, 0) /* Square */  => (32, 32),
            (3, 0) /* Square */  => (64, 64),
            (0, 1) /* Wide */  => (16, 8),
            (1, 1) /* Wide */  => (32, 8),
            (2, 1) /* Wide */  => (32, 16),
            (3, 1) /* Wide */  => (64, 32),
            (0, 2) /* Tall */  => (8, 16),
            (1, 2) /* Tall */  => (8, 32),
            (2, 2) /* Tall */  => (16, 32),
            (3, 2) /* Tall */  => (32, 64),
            _ => (8, 8), // according to commit f01016a30b2e8482d06798895ebc674370e81816 in melonDS
        }
    }
    fn coords(&self) -> (usize, usize) {
        (self.1.x_coord() as usize, self.0.y_coord() as usize)
    }
    fn tile_format(&self) -> (usize, PixelFormat) {
        if self.0.is_8bpp() {
            (0x40, PixelFormat::BPP8)
        } else {
            (0x20, PixelFormat::BPP4)
        }
    }
    fn is_affine(&self) -> bool {
        match self.0.objtype() {
            ObjType::Affine | ObjType::AffineDoubleSize => true,
            _ => false,
        }
    }
    fn affine_index(&self) -> u32 {
        let attr1 = (self.1).0;
        ((attr1 >> 9) & 0x1f) as u32
    }
    fn is_hidden(&self) -> bool {
        self.0.objtype() == ObjType::Hidden
    }
    fn flip_xy(&self) -> (bool, bool) {
        if !self.is_affine() {
            (self.1.h_flip(), self.1.v_flip())
        } else {
            (false, false)
        }
    }
}

fn read_obj_attrs(sb: &SysBus, obj: usize) -> ObjAttrs {
    let addr = ATTRS_SIZE * (obj as u32);
    let attr0 = Attribute0(sb.oam.read_16(addr + 0));
    let attr1 = Attribute1(sb.oam.read_16(addr + 2));
    let attr2 = Attribute2(sb.oam.read_16(addr + 4));
    ObjAttrs(attr0, attr1, attr2)
}

impl Gpu {
    fn obj_tile_base(&self) -> u32 {
        match self.dispcnt.mode() {
            mode if mode > 2 => OVRAM + 0x4000,
            _ => OVRAM,
        }
    }

    pub fn render_objs(&mut self, sb: &SysBus) {
        let screen_y = self.current_scanline;
        // reset the scanline
        self.obj_line = Scanline::default();
        self.obj_line_priorities = Scanline([3; DISPLAY_WIDTH]);
        for obj_num in (0..128).rev() {
            let obj = read_obj_attrs(sb, obj_num);
            if obj.is_hidden() {
                continue;
            }

            let is_affine = obj.is_affine();
            if is_affine {
                panic!("im not ready for that yet :(");
            }

            let (obj_x, obj_y) = obj.coords();
            let (obj_w, obj_h) = obj.size();
            // skip this obj if not within its bounds.
            if !(screen_y >= obj_y && screen_y < obj_y + obj_h) {
                continue;
            }

            let tile_base = self.obj_tile_base() + 0x20 * (obj.2.tile() as u32);

            let (tile_size, pixel_format) = obj.tile_format();
            let palette_bank = match pixel_format {
                PixelFormat::BPP4 => obj.2.palette(),
                _ => 0u32,
            };

            let tile_array_width = match self.dispcnt.obj_mapping() {
                ObjMapping::OneDimension => obj_w / 8,
                ObjMapping::TwoDimension => {
                    if obj.0.is_8bpp() {
                        16
                    } else {
                        32
                    }
                }
            };

            let (xflip, yflip) = obj.flip_xy();

            let end_x = obj_x + obj_w;
            for screen_x in obj_x..end_x {
                if screen_x > DISPLAY_WIDTH {
                    break;
                }
                if self.obj_line_priorities[screen_x] < obj.2.priority() {
                    continue;
                }
                let mut sprite_y = screen_y - obj_y;
                let mut sprite_x = screen_x - obj_x;
                if (!is_affine) {
                    sprite_y = if yflip {
                        obj_h - sprite_y - 1
                    } else {
                        sprite_y
                    };
                    sprite_x = if xflip {
                        obj_w - sprite_x - 1
                    } else {
                        sprite_x
                    };
                }
                let tile_x = sprite_x % 8;
                let tile_y = sprite_y % 8;
                let tile_addr = tile_base
                    + index2d!(u32, sprite_x / 8, sprite_y / 8, tile_array_width)
                        * (tile_size as u32);
                let pixel_index = self.read_pixel_index(
                    sb,
                    tile_addr,
                    tile_x as u32,
                    tile_y as u32,
                    pixel_format,
                );
                let pixel_color =
                    self.get_palette_color(sb, pixel_index as u32, palette_bank, PALRAM_OFS_FG);
                if pixel_color != Rgb15::TRANSPARENT {
                    self.obj_line[screen_x] = pixel_color;
                    self.obj_line_priorities[screen_x] = obj.2.priority();
                }
            }
        }
    }
}

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
enum ObjMode {
    Normal = 0b00,
    Sfx = 0b01,
    Window = 0b10,
    Forbidden = 0b11,
}

impl From<u16> for ObjMode {
    fn from(v: u16) -> ObjMode {
        ObjMode::from_u16(v as u16).unwrap()
    }
}

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
enum ObjType {
    Normal = 0b00,
    Affine = 0b01,
    Hidden = 0b10,
    AffineDoubleSize = 0b11,
}

impl From<u16> for ObjType {
    fn from(v: u16) -> ObjType {
        ObjType::from_u16(v as u16).unwrap()
    }
}

bitfield! {
    pub struct Attribute0(u16);
    u16;
    y_coord, _ : 7, 0;
    into ObjType, objtype, _: 9, 8;
    into ObjMode, objmode, _: 11, 10;
    pub mosaic, _: 12;
    is_8bpp, _: 13;
    shape, _: 15, 14;
}

bitfield! {
    pub struct Attribute1(u16);
    u16;
    x_coord, _ : 8, 0;
    h_flip, _: 12;
    v_flip, _: 13;
    size, _: 15, 14;
}

bitfield! {
    pub struct Attribute2(u16);
    u16;
    tile, _: 9, 0;
    priority, _: 11, 10;
    into u32, palette, _: 15, 12;
}
