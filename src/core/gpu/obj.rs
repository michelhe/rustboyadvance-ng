use super::super::SysBus;
use super::regs::*;
use super::*;

const OVRAM: u32 = 0x0601_0000;
const PALRAM_OFS_FG: u32 = 0x200;
const ATTRS_SIZE: u32 = 2 * 3 + 2;

struct ObjAttrs(Attribute0, Attribute1, Attribute2);

const AFFINE_FILL: u32 = 2 * 3;

impl AffineMatrix {
    fn from_index(sb: &SysBus, index: u32) -> AffineMatrix {
        let mut offset = AFFINE_FILL + index * 16 * 2;
        let pa = sb.oam.read_16(offset) as i16 as i32;
        offset += 2 + AFFINE_FILL;
        let pb = sb.oam.read_16(offset) as i16 as i32;
        offset += 2 + AFFINE_FILL;
        let pc = sb.oam.read_16(offset) as i16 as i32;
        offset += 2 + AFFINE_FILL;
        let pd = sb.oam.read_16(offset) as i16 as i32;

        AffineMatrix { pa, pb, pc, pd }
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
    fn coords(&self) -> (i32, i32) {
        let mut y = self.0.y_coord() as i16 as i32;
        let mut x = self.1.x_coord() as i16 as i32;
        if y >= (DISPLAY_HEIGHT as i32) {
            y -= 1 << 8;
        }
        if x >= (DISPLAY_WIDTH as i32) {
            x -= 1 << 9;
        }
        (x, y)
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
    fn affine_matrix(&self, sb: &SysBus) -> AffineMatrix {
        AffineMatrix::from_index(sb, self.affine_index())
    }
    fn is_hidden(&self) -> bool {
        self.0.objtype() == ObjType::Hidden
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

    fn render_affine_obj(&mut self, sb: &SysBus, obj: ObjAttrs, _obj_num: usize) {
        let screen_y = self.vcount as i32;

        let (ref_x, ref_y) = obj.coords();

        let (obj_w, obj_h) = obj.size();

        let (bbox_w, bbox_h) = match obj.0.objtype() {
            ObjType::AffineDoubleSize => (2 * obj_w, 2 * obj_h),
            _ => (obj_w, obj_h),
        };

        // skip this obj if not within its vertical bounds.
        if !(screen_y >= ref_y && screen_y < ref_y + bbox_h) {
            return;
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

        let affine_matrix = obj.affine_matrix(sb);

        let half_width = bbox_w / 2;
        let half_height = bbox_h / 2;
        let screen_width = DISPLAY_WIDTH as i32;
        let iy = screen_y - (ref_y + half_height);
        for ix in (-half_width)..(half_width) {
            let screen_x = ref_x + half_width + ix;
            if screen_x < 0 {
                continue;
            }
            if screen_x >= screen_width {
                break;
            }
            if self.obj_buffer[screen_x as usize].priority <= obj.2.priority() {
                continue;
            }

            let transformed_x = (affine_matrix.pa * ix + affine_matrix.pb * iy) >> 8;
            let transformed_y = (affine_matrix.pc * ix + affine_matrix.pd * iy) >> 8;
            let texture_x = transformed_x + obj_w / 2;
            let texture_y = transformed_y + obj_h / 2;
            if texture_x >= 0 && texture_x < obj_w && texture_y >= 0 && texture_y < obj_h {
                let tile_x = texture_x % 8;
                let tile_y = texture_y % 8;
                let tile_addr = tile_base
                    + index2d!(u32, texture_x / 8, texture_y / 8, tile_array_width)
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
                    let mut current_obj = &mut self.obj_buffer[screen_x as usize];
                    current_obj.color = pixel_color;
                    current_obj.priority = obj.2.priority();
                }
            }
        }
    }

    fn render_normal_obj(&mut self, sb: &SysBus, obj: ObjAttrs, _obj_num: usize) {
        let screen_y = self.vcount as i32;

        let (ref_x, ref_y) = obj.coords();
        let (obj_w, obj_h) = obj.size();

        // skip this obj if not within its vertical bounds.
        if !(screen_y >= ref_y && screen_y < ref_y + obj_h) {
            return;
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

        // render the pixels
        let screen_width = DISPLAY_WIDTH as i32;
        let end_x = ref_x + obj_w;
        for screen_x in ref_x..end_x {
            if screen_x < 0 {
                continue;
            }
            if screen_x >= screen_width {
                break;
            }
            if self.obj_buffer[screen_x as usize].priority <= obj.2.priority() {
                continue;
            }
            let mut sprite_y = screen_y - ref_y;
            let mut sprite_x = screen_x - ref_x;
            sprite_y = if obj.1.v_flip() {
                obj_h - sprite_y - 1
            } else {
                sprite_y
            };
            sprite_x = if obj.1.h_flip() {
                obj_w - sprite_x - 1
            } else {
                sprite_x
            };
            let tile_x = sprite_x % 8;
            let tile_y = sprite_y % 8;
            let tile_addr = tile_base
                + index2d!(u32, sprite_x / 8, sprite_y / 8, tile_array_width) * (tile_size as u32);
            let pixel_index =
                self.read_pixel_index(sb, tile_addr, tile_x as u32, tile_y as u32, pixel_format);
            let pixel_color =
                self.get_palette_color(sb, pixel_index as u32, palette_bank, PALRAM_OFS_FG);
            if pixel_color != Rgb15::TRANSPARENT {
                let mut current_obj = &mut self.obj_buffer[screen_x as usize];
                current_obj.color = pixel_color;
                current_obj.priority = obj.2.priority();
            }
        }
    }

    pub fn render_objs(&mut self, sb: &SysBus) {
        // reset the scanline
        self.obj_buffer = [Default::default(); DISPLAY_WIDTH];
        for obj_num in 0..128 {
            let obj = read_obj_attrs(sb, obj_num);
            match obj.0.objtype() {
                ObjType::Hidden => continue,
                ObjType::Normal => self.render_normal_obj(sb, obj, obj_num),
                ObjType::Affine | ObjType::AffineDoubleSize => {
                    self.render_affine_obj(sb, obj, obj_num)
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ObjInfo {
    pub(super) color: Rgb15,
    pub(super) priority: u16,
    pub(super) mode: ObjMode,
}

impl Default for ObjInfo {
    fn default() -> ObjInfo {
        ObjInfo {
            mode: ObjMode::Normal,
            color: Rgb15::TRANSPARENT,
            priority: 4,
        }
    }
}

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
pub enum ObjMode {
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
