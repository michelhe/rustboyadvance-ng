use std::cmp;
use std::ops::Sub;

use super::regs::*;
use super::*;

#[derive(Debug, Primitive, Clone, Copy)]
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

#[derive(Debug, Default)]
struct Layer {
    color: Rgb15,
    blend_flag: BlendFlags,
}

impl Gpu {
    fn get_top_layer(
        &self,
        sb: &SysBus,
        screen_x: usize,
        bflags: BlendFlags,
        wflags: WindowFlags,
    ) -> Option<Layer> {
        // priorities are 0-4 when 0 is the highest
        'outer: for priority in 0..4 {
            if bflags.contains(BlendFlags::OBJ)
                && wflags.contains(WindowFlags::OBJ)
                && !self.obj_line[screen_x].is_transparent()
                && self.obj_line_priorities[screen_x] == priority
            {
                return Some(Layer {
                    color: self.obj_line[screen_x],
                    blend_flag: BlendFlags::OBJ,
                });
            }
            for bg in 0..4 {
                let c = self.bg[bg].line[screen_x];
                let bflag = BlendFlags::from_bg(bg);
                if self.dispcnt.disp_bg(bg)
                    && !c.is_transparent()
                    && bflags.contains(bflag)
                    && wflags.bg_enabled(bg)
                    && self.bg[bg].bgcnt.priority() == priority
                {
                    return Some(Layer {
                        color: c,
                        blend_flag: bflag,
                    });
                }
            }
        }
        if bflags.contains(BlendFlags::BACKDROP) {
            return Some(Layer {
                color: Rgb15(sb.palette_ram.read_16(0)),
                blend_flag: BlendFlags::BACKDROP,
            });
        }
        None
    }

    fn get_active_window_type(&self, x: usize, y: usize) -> WindowType {
        if !self.dispcnt.is_using_windows() {
            WindowType::WinNone
        } else {
            if self.dispcnt.disp_window0() && self.win0.inside(x, y) {
                return WindowType::Win0;
            }
            if self.dispcnt.disp_window1() && self.win1.inside(x, y) {
                return WindowType::Win1;
            }
            // TODO win_obj
            return WindowType::WinOut;
        }
    }

    fn get_window_flags(&self, wintyp: WindowType) -> WindowFlags {
        match wintyp {
            WindowType::Win0 => self.win0.flags,
            WindowType::Win1 => self.win1.flags,
            WindowType::WinObj => self.winobj_flags,
            WindowType::WinOut => self.winout_flags,
            WindowType::WinNone => WindowFlags::all(),
        }
    }

    fn sfx_blend_alpha(
        &self,
        sb: &SysBus,
        x: usize,
        _y: usize,
        wflags: WindowFlags,
    ) -> Option<Rgb15> {
        let top_layers = self.bldcnt.top();
        let bottom_layers = self.bldcnt.bottom();
        if let Some(top_layer) = self.get_top_layer(sb, x, top_layers, wflags) {
            if let Some(bot_layer) = self.get_top_layer(sb, x, bottom_layers, wflags) {
                let eva = self.bldalpha.eva();
                let evb = self.bldalpha.evb();
                return Some(top_layer.color.blend_with(bot_layer.color, eva, evb));
            } else {
                return Some(top_layer.color);
            }
        }
        None
    }

    fn sfx_blend_bw(
        &self,
        sb: &SysBus,
        fadeto: Rgb15,
        x: usize,
        _y: usize,
        wflags: WindowFlags,
    ) -> Option<Rgb15> {
        let top_layers = self.bldcnt.top();
        let evy = self.bldy;

        if let Some(layer) = self.get_top_layer(sb, x, top_layers, wflags) {
            return Some(layer.color.blend_with(fadeto, 16 - evy, evy));
        }
        None
    }

    pub fn composite_sfx(&self, sb: &SysBus) -> Scanline<Rgb15> {
        let mut line: Scanline<Rgb15> = Scanline::default();
        let y = self.vcount;
        for x in 0..DISPLAY_WIDTH {
            let window = self.get_active_window_type(x, y);
            let wflags = self.get_window_flags(window);
            let toplayer = self
                .get_top_layer(sb, x, BlendFlags::all(), wflags)
                .unwrap();

            let bldmode = if wflags.sfx_enabled() {
                self.bldcnt.mode()
            } else {
                BldMode::BldNone
            };

            match bldmode {
                BldMode::BldAlpha => {
                    if self.bldcnt.top().contains(toplayer.blend_flag)
                        || self.bldcnt.bottom().contains(toplayer.blend_flag)
                    {
                        line[x] = self
                            .sfx_blend_alpha(sb, x, y, wflags)
                            .unwrap_or(toplayer.color);
                    } else {
                        line[x] = toplayer.color;
                    }
                }
                BldMode::BldWhite => {
                    let result = if self.bldcnt.top().contains(toplayer.blend_flag) {
                        self.sfx_blend_bw(sb, Rgb15::WHITE, x, y, wflags)
                            .unwrap_or(toplayer.color)
                    } else {
                        toplayer.color
                    };
                    line[x] = result;
                }
                BldMode::BldBlack => {
                    let result = if self.bldcnt.top().contains(toplayer.blend_flag) {
                        self.sfx_blend_bw(sb, Rgb15::BLACK, x, y, wflags)
                            .unwrap_or(toplayer.color)
                    } else {
                        toplayer.color
                    };
                    line[x] = result;
                }
                BldMode::BldNone => {
                    line[x] = toplayer.color;
                }
            }
        }
        line
    }
}
