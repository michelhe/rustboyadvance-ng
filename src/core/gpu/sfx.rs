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

impl Gpu {
    fn get_topmost_color(
        &self,
        sb: &SysBus,
        screen_x: usize,
        layers: &BlendFlags,
    ) -> Option<Rgb15> {
        // TODO - only BGs are supported, don't forget OBJs

        let mut color: Option<Rgb15> = None;

        // priorities are 0-4 when 0 is the highest
        'outer: for priority in 0..4 {
            for bg in 0..4 {
                let c = self.bg[bg].line[screen_x];
                if self.dispcnt.disp_bg(bg)
                    && !c.is_transparent()
                    && (layers.is_empty() || layers.contains(BG_LAYER_FLAG[bg]))
                    && self.bg[bg].bgcnt.priority() == priority
                {
                    color = Some(c);
                    break 'outer;
                }
            }
        }
        if color.is_none() && layers.contains(BlendFlags::BACKDROP) {
            color = Some(Rgb15(sb.palette_ram.read_16(0)))
        }
        color
    }

    pub fn blend_line(&mut self, sb: &mut SysBus) -> Scanline {
        let mut bld_line = Scanline::default();
        let bldmode = self.bldcnt.mode();
        match bldmode {
            BldMode::BldAlpha => {
                let top_layers = self.bldcnt.top();
                let bottom_layers = self.bldcnt.bottom();

                for x in 0..DISPLAY_WIDTH {
                    if let Some(top_color) = self.get_topmost_color(sb, x, &top_layers) {
                        if let Some(bot_color) = self.get_topmost_color(sb, x, &bottom_layers) {
                            let eva = self.bldalpha.eva();
                            let evb = self.bldalpha.evb();
                            bld_line[x] = top_color.blend_with(bot_color, eva, evb);
                        } else {
                            bld_line[x] = top_color;
                        }
                    } else {
                        bld_line[x] = self.get_topmost_color(sb, x, &BlendFlags::all()).unwrap();
                    }
                }
            }
            BldMode::BldWhite => {
                let top_layers = self.bldcnt.top();
                let evy = self.bldy;

                for x in 0..DISPLAY_WIDTH {
                    bld_line[x] =
                        if let Some(top_color) = self.get_topmost_color(sb, x, &top_layers) {
                            top_color.blend_with(Rgb15::WHITE, 16 - evy, evy)
                        } else {
                            self.get_topmost_color(sb, x, &BlendFlags::all()).unwrap()
                        };
                }
            }
            BldMode::BldBlack => {
                let top_layers = self.bldcnt.top();
                let evy = self.bldy;

                for x in 0..DISPLAY_WIDTH {
                    bld_line[x] =
                        if let Some(top_color) = self.get_topmost_color(sb, x, &top_layers) {
                            top_color.blend_with(Rgb15::BLACK, 16 - evy, evy)
                        } else {
                            self.get_topmost_color(sb, x, &BlendFlags::all()).unwrap()
                        };
                }
            }
            BldMode::BldNone => {
                for x in 0..DISPLAY_WIDTH {
                    bld_line[x] = self.get_topmost_color(sb, x, &BlendFlags::all()).unwrap();
                }
            }
        }
        bld_line
    }
}
