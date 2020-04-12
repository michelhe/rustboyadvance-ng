use std::cmp;

use arrayvec::ArrayVec;
use num::FromPrimitive;

use super::regs::*;

use super::layer::*;
use super::*;

#[derive(Debug, Primitive, PartialEq, Clone, Copy)]
pub enum BldMode {
    BldNone = 0b00,
    BldAlpha = 0b01,
    BldWhite = 0b10,
    BldBlack = 0b11,
}

impl From<u16> for BldMode {
    fn from(v: u16) -> BldMode {
        BldMode::from_u16(v).unwrap()
    }
}

impl Rgb15 {
    fn blend_with(self, other: Rgb15, my_weight: u16, other_weight: u16) -> Rgb15 {
        let r = cmp::min(31, (self.r() * my_weight + other.r() * other_weight) >> 4);
        let g = cmp::min(31, (self.g() * my_weight + other.g() * other_weight) >> 4);
        let b = cmp::min(31, (self.b() * my_weight + other.b() * other_weight) >> 4);
        Rgb15::from_rgb(r, g, b)
    }
}

impl From<WindowFlags> for BlendFlags {
    fn from(wf: WindowFlags) -> BlendFlags {
        BlendFlags::from_bits(wf.bits()).unwrap()
    }
}

impl Gpu {
    /// Returns background indexes in render order. Filters range by bg_start..=bg_end.
    fn sorted_backgrounds(&self, bg_start: usize, bg_end: usize) -> ArrayVec<[usize; 4]> {
        let mut backgrounds: ArrayVec<[usize; 4]> = (bg_start..=bg_end).collect();
        backgrounds.sort_by_key(|bg| (self.backgrounds[*bg].bgcnt.priority(), *bg));
        backgrounds
    }

    /// Filters a background indexes array by whether they're active
    fn active_backgrounds(
        &self,
        backgrounds: &[usize],
        window_flags: WindowFlags,
    ) -> ArrayVec<[usize; 4]> {
        backgrounds
            .iter()
            .copied()
            .filter(|bg| self.dispcnt.enable_bg(*bg) && window_flags.bg_enabled(*bg))
            .collect()
    }

    #[allow(unused)]
    fn layer_to_pixel(&self, x: usize, y: usize, layer: &RenderLayer) -> Rgb15 {
        match layer.kind {
            RenderLayerKind::Background0 => self.backgrounds[0].line[x],
            RenderLayerKind::Background1 => self.backgrounds[1].line[x],
            RenderLayerKind::Background2 => self.backgrounds[2].line[x],
            RenderLayerKind::Background3 => self.backgrounds[3].line[x],
            RenderLayerKind::Objects => self.obj_buffer_get(x, y).color,
            RenderLayerKind::Backdrop => Rgb15(self.palette_ram.read_16(0)),
        }
    }

    /// Composes the render layers into a final scanline while applying needed special effects, and render it to the frame buffer
    pub fn finalize_scanline(&self, frame_buffer: &mut [u32], bg_start: usize, bg_end: usize) {
        let backdrop_color = Rgb15(self.palette_ram.read_16(0));
        let sorted_backgrounds = self.sorted_backgrounds(bg_start, bg_end);

        let y = self.vcount;
        let start_index = y * DISPLAY_WIDTH;
        let output = &mut frame_buffer[start_index..start_index + DISPLAY_WIDTH];

        if !self.dispcnt.is_using_windows() {
            let win = WindowInfo::new(WindowType::WinNone, WindowFlags::all());
            let backgrounds = self.active_backgrounds(&sorted_backgrounds, win.flags);
            for x in 0..DISPLAY_WIDTH {
                let pixel = self.compose_pixel(x, y, &win, &backgrounds, backdrop_color);
                output[x] = pixel.to_rgb24();
            }
        } else {
            let mut occupied = [false; DISPLAY_WIDTH];
            let mut occupied_count = 0;
            if self.dispcnt.enable_window0() && self.win0.contains_y(y) {
                let win = WindowInfo::new(WindowType::Win0, self.win0.flags);
                let backgrounds = self.active_backgrounds(&sorted_backgrounds, win.flags);
                for x in self.win0.left()..self.win0.right() {
                    let pixel = self.compose_pixel(x, y, &win, &backgrounds, backdrop_color);
                    output[x] = pixel.to_rgb24();
                    occupied[x] = true;
                    occupied_count += 1;
                }
            }
            if occupied_count == DISPLAY_WIDTH {
                return;
            }
            if self.dispcnt.enable_window1() && self.win1.contains_y(y) {
                let win = WindowInfo::new(WindowType::Win1, self.win1.flags);
                let backgrounds = self.active_backgrounds(&sorted_backgrounds, win.flags);
                for x in self.win1.left()..self.win1.right() {
                    if !occupied[x] {
                        let pixel = self.compose_pixel(x, y, &win, &backgrounds, backdrop_color);
                        output[x] = pixel.to_rgb24();
                        occupied[x] = true;
                        occupied_count += 1;
                    }
                }
            }
            if occupied_count == DISPLAY_WIDTH {
                return;
            }
            let win_out = WindowInfo::new(WindowType::WinOut, self.winout_flags);
            let win_out_backgrounds = self.active_backgrounds(&sorted_backgrounds, win_out.flags);
            if self.dispcnt.enable_obj_window() {
                let win_obj = WindowInfo::new(WindowType::WinObj, self.winobj_flags);
                let win_obj_backgrounds =
                    self.active_backgrounds(&sorted_backgrounds, win_obj.flags);
                for x in 0..DISPLAY_WIDTH {
                    if occupied[x] {
                        continue;
                    }
                    let obj_entry = self.obj_buffer_get(x, y);
                    if obj_entry.window {
                        // WinObj
                        let pixel = self.compose_pixel(
                            x,
                            y,
                            &win_obj,
                            &win_obj_backgrounds,
                            backdrop_color,
                        );
                        output[x] = pixel.to_rgb24();
                        occupied[x] = true;
                        occupied_count += 1;
                    } else {
                        // WinOut
                        let pixel = self.compose_pixel(
                            x,
                            y,
                            &win_out,
                            &win_out_backgrounds,
                            backdrop_color,
                        );
                        output[x] = pixel.to_rgb24();
                        occupied[x] = true;
                        occupied_count += 1;
                    }
                }
            } else {
                for x in 0..DISPLAY_WIDTH {
                    if occupied[x] {
                        continue;
                    }
                    let pixel =
                        self.compose_pixel(x, y, &win_out, &win_out_backgrounds, backdrop_color);
                    output[x] = pixel.to_rgb24();
                    occupied[x] = true;
                    occupied_count += 1;
                }
            }
        }
    }

    fn compose_pixel(
        &self,
        x: usize,
        y: usize,
        win: &WindowInfo,
        backgrounds: &[usize],
        backdrop_color: Rgb15,
    ) -> Rgb15 {
        let mut layers = ArrayVec::<[_; 7]>::new();
        unsafe {
            layers.push_unchecked(RenderLayer::backdrop(backdrop_color));
        }

        for bg in backgrounds.iter() {
            let bg_pixel = self.backgrounds[*bg].line[x];
            if !bg_pixel.is_transparent() {
                unsafe {
                    layers.push_unchecked(RenderLayer::background(
                        *bg,
                        bg_pixel,
                        self.backgrounds[*bg].bgcnt.priority(),
                    ));
                }
            }
        }

        let obj_entry = self.obj_buffer_get(x, y);
        if self.dispcnt.enable_obj() && win.flags.obj_enabled() && !obj_entry.color.is_transparent()
        {
            unsafe {
                layers.push_unchecked(RenderLayer::objects(obj_entry.color, obj_entry.priority))
            }
        }

        let top_layer = layers.iter().min().unwrap();
        let top_pixel = top_layer.pixel; // self.layer_to_pixel(x, y, &top_layer);

        let obj_sfx = obj_entry.alpha && top_layer.is_object();
        if !win.flags.sfx_enabled() && !obj_sfx {
            return top_pixel;
        }

        let top_layer_flags = self.bldcnt.top();
        let bot_layer_flags = self.bldcnt.bottom();

        if !(top_layer_flags.contains_render_layer(&top_layer) || obj_sfx) {
            return top_pixel;
        }

        // TODO: get layers[1] without sorting them all
        layers.sort();
        if layers.len() > 1 && !(bot_layer_flags.contains_render_layer(&layers[1])) {
            return top_pixel;
        }

        let mut blend_mode = self.bldcnt.mode();

        // push another backdrop layer in case there is only 1 layer
        // unsafe { layers.push_unchecked(RenderLayer::backdrop(backdrop_color)); }
        // if this is object alpha blending, ensure that the bottom layer contains a color to blend with
        if obj_sfx && layers.len() > 1 && bot_layer_flags.contains_render_layer(&layers[1]) {
            blend_mode = BldMode::BldAlpha;
        }

        match blend_mode {
            BldMode::BldAlpha => {
                let bot_pixel = if layers.len() > 1 {
                    layers[1].pixel //self.layer_to_pixel(x, y, &layers[1])
                } else {
                    backdrop_color
                };

                let eva = self.bldalpha.eva();
                let evb = self.bldalpha.evb();
                top_pixel.blend_with(bot_pixel, eva, evb)
            }
            BldMode::BldWhite => {
                let evy = self.bldy;
                top_pixel.blend_with(Rgb15::WHITE, 16 - evy, evy)
            }
            BldMode::BldBlack => {
                let evy = self.bldy;
                top_pixel.blend_with(Rgb15::BLACK, 16 - evy, evy)
            }
            BldMode::BldNone => top_pixel,
        }
    }
}
