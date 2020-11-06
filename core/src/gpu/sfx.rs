use std::cmp;

use arrayvec::ArrayVec;

use super::regs::*;

use super::layer::*;
use super::*;

impl Rgb15 {
    fn blend_with(self, other: Rgb15, my_weight: u16, other_weight: u16) -> Rgb15 {
        let r = cmp::min(31, (self.r() * my_weight + other.r() * other_weight) >> 4);
        let g = cmp::min(31, (self.g() * my_weight + other.g() * other_weight) >> 4);
        let b = cmp::min(31, (self.b() * my_weight + other.b() * other_weight) >> 4);
        Rgb15::from_rgb(r, g, b)
    }
}

/// Filters a background indexes array by whether they're active
fn filter_window_backgrounds(
    backgrounds: &[usize],
    window_flags: WindowFlags,
) -> ArrayVec<[usize; 4]> {
    backgrounds
        .iter()
        .copied()
        .filter(|bg| window_flags.bg_enabled(*bg))
        .collect()
}

impl Gpu {
    #[allow(unused)]
    fn layer_to_pixel(&mut self, x: usize, y: usize, layer: &RenderLayer) -> Rgb15 {
        match layer.kind {
            RenderLayerKind::Background0 => self.bg_line[0][x],
            RenderLayerKind::Background1 => self.bg_line[1][x],
            RenderLayerKind::Background2 => self.bg_line[2][x],
            RenderLayerKind::Background3 => self.bg_line[3][x],
            RenderLayerKind::Objects => self.obj_buffer_get(x, y).color,
            RenderLayerKind::Backdrop => Rgb15(self.palette_ram.read_16(0)),
        }
    }

    /// Composes the render layers into a final scanline while applying needed special effects, and render it to the frame buffer
    pub fn finalize_scanline(&mut self, bg_start: usize, bg_end: usize) {
        let backdrop_color = Rgb15(self.palette_ram.read_16(0));

        // filter out disabled backgrounds and sort by priority
        // the backgrounds are sorted once for the entire scanline
        let mut sorted_backgrounds: ArrayVec<[usize; 4]> = (bg_start..=bg_end)
            .filter(|bg| self.dispcnt.enable_bg[*bg])
            .collect();
        sorted_backgrounds.sort_by_key(|bg| (self.bgcnt[*bg].priority, *bg));

        let y = self.vcount;

        if !self.dispcnt.is_using_windows() {
            for x in 0..DISPLAY_WIDTH {
                let win = WindowInfo::new(WindowType::WinNone, WindowFlags::all());
                self.finalize_pixel(x, y, &win, &sorted_backgrounds, backdrop_color);
            }
        } else {
            let mut occupied = [false; DISPLAY_WIDTH];
            let mut occupied_count = 0;
            if self.dispcnt.enable_window0 && self.win0.contains_y(y) {
                let win = WindowInfo::new(WindowType::Win0, self.win0.flags);
                let backgrounds = filter_window_backgrounds(&sorted_backgrounds, win.flags);
                for x in self.win0.left()..self.win0.right() {
                    self.finalize_pixel(x, y, &win, &backgrounds, backdrop_color);
                    occupied[x] = true;
                    occupied_count += 1;
                }
            }
            if occupied_count == DISPLAY_WIDTH {
                return;
            }
            if self.dispcnt.enable_window1 && self.win1.contains_y(y) {
                let win = WindowInfo::new(WindowType::Win1, self.win1.flags);
                let backgrounds = filter_window_backgrounds(&sorted_backgrounds, win.flags);
                for x in self.win1.left()..self.win1.right() {
                    if occupied[x] {
                        continue;
                    }
                    self.finalize_pixel(x, y, &win, &backgrounds, backdrop_color);
                    occupied[x] = true;
                    occupied_count += 1;
                }
            }
            if occupied_count == DISPLAY_WIDTH {
                return;
            }
            let win_out = WindowInfo::new(WindowType::WinOut, self.winout_flags);
            let win_out_backgrounds = filter_window_backgrounds(&sorted_backgrounds, win_out.flags);
            if self.dispcnt.enable_obj_window {
                let win_obj = WindowInfo::new(WindowType::WinObj, self.winobj_flags);
                let win_obj_backgrounds =
                    filter_window_backgrounds(&sorted_backgrounds, win_obj.flags);
                for x in 0..DISPLAY_WIDTH {
                    if occupied[x] {
                        continue;
                    }
                    let obj_entry = self.obj_buffer_get(x, y);
                    if obj_entry.window {
                        // WinObj
                        self.finalize_pixel(x, y, &win_obj, &win_obj_backgrounds, backdrop_color);
                    } else {
                        // WinOut
                        self.finalize_pixel(x, y, &win_out, &win_out_backgrounds, backdrop_color);
                    }
                }
            } else {
                for x in 0..DISPLAY_WIDTH {
                    if occupied[x] {
                        continue;
                    }
                    self.finalize_pixel(x, y, &win_out, &win_out_backgrounds, backdrop_color);
                }
            }
        }
    }

    fn finalize_pixel(
        &mut self,
        x: usize,
        y: usize,
        win: &WindowInfo,
        backgrounds: &[usize],
        backdrop_color: Rgb15,
    ) {
        let output = unsafe {
            let ptr = self.frame_buffer[y * DISPLAY_WIDTH..].as_mut_ptr();
            std::slice::from_raw_parts_mut(ptr, DISPLAY_WIDTH)
        };

        // The backdrop layer is the default
        let backdrop_layer = RenderLayer::backdrop(backdrop_color);

        // Backgrounds are already sorted
        // lets start by taking the first 2 backgrounds that have an opaque pixel at x
        let mut it = backgrounds
            .iter()
            .filter(|i| !self.bg_line[**i][x].is_transparent())
            .take(2);

        let mut top_layer = it.next().map_or(backdrop_layer, |bg| {
            RenderLayer::background(*bg, self.bg_line[*bg][x], self.bgcnt[*bg].priority)
        });

        let mut bot_layer = it.next().map_or(backdrop_layer, |bg| {
            RenderLayer::background(*bg, self.bg_line[*bg][x], self.bgcnt[*bg].priority)
        });

        drop(it);

        // Now that backgrounds are taken care of, we need to check if there is an object pixel that takes priority of one of the layers
        let obj_entry = self.obj_buffer_get(x, y);
        if win.flags.obj_enabled() && self.dispcnt.enable_obj && !obj_entry.color.is_transparent() {
            let obj_layer = RenderLayer::objects(obj_entry.color, obj_entry.priority);
            if obj_layer.priority <= top_layer.priority {
                bot_layer = top_layer;
                top_layer = obj_layer;
            } else if obj_layer.priority <= bot_layer.priority {
                bot_layer = obj_layer;
            }
        }

        let obj_entry = self.obj_buffer_get(x, y);
        let obj_alpha_blend = top_layer.is_object() && obj_entry.alpha;

        let top_flags = self.bldcnt.target1;
        let bot_flags = self.bldcnt.target2;

        let sfx_enabled = (self.bldcnt.mode != BlendMode::BldNone || obj_alpha_blend)
            && top_flags.contains_render_layer(&top_layer); // sfx must at least have a first target configured

        if win.flags.sfx_enabled() && sfx_enabled {
            if top_layer.is_object()
                && obj_alpha_blend
                && bot_flags.contains_render_layer(&bot_layer)
            {
                output[x] = self.do_alpha(top_layer.pixel, bot_layer.pixel).to_rgb24();
            } else {
                let (top_layer, bot_layer) = (top_layer, bot_layer);

                match self.bldcnt.mode {
                    BlendMode::BldAlpha => {
                        output[x] = if bot_flags.contains_render_layer(&bot_layer) {
                            self.do_alpha(top_layer.pixel, bot_layer.pixel).to_rgb24()
                        } else {
                            // alpha blending must have a 2nd target
                            top_layer.pixel.to_rgb24()
                        }
                    }
                    BlendMode::BldWhite => output[x] = self.do_brighten(top_layer.pixel).to_rgb24(),

                    BlendMode::BldBlack => output[x] = self.do_darken(top_layer.pixel).to_rgb24(),

                    BlendMode::BldNone => output[x] = top_layer.pixel.to_rgb24(),
                }
            }
        } else {
            // no blending, just use the top pixel
            output[x] = top_layer.pixel.to_rgb24();
        }
    }

    #[inline]
    fn do_alpha(&self, upper: Rgb15, lower: Rgb15) -> Rgb15 {
        let eva = self.bldalpha.eva;
        let evb = self.bldalpha.evb;
        upper.blend_with(lower, eva, evb)
    }

    #[inline]
    fn do_brighten(&self, c: Rgb15) -> Rgb15 {
        let evy = self.bldy;
        c.blend_with(Rgb15::WHITE, 16 - evy, evy)
    }

    #[inline]
    fn do_darken(&self, c: Rgb15) -> Rgb15 {
        let evy = self.bldy;
        c.blend_with(Rgb15::BLACK, 16 - evy, evy)
    }
}
