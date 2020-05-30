pub(super) mod bitmap;
pub(super) mod obj;
pub(super) mod text;

pub(super) type Point = (i32, i32);

#[derive(Debug)]
pub(super) struct ViewPort {
    pub origin: Point,
    pub w: i32,
    pub h: i32,
}

impl ViewPort {
    pub fn new(w: i32, h: i32) -> ViewPort {
        ViewPort {
            origin: (0, 0),
            w: w,
            h: h,
        }
    }

    pub fn contains_point(&self, p: Point) -> bool {
        let (mut x, mut y) = p;

        x -= self.origin.0;
        y -= self.origin.1;

        x >= 0 && x < self.w && y >= 0 && y < self.h
    }
}

use super::consts::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
pub(super) static SCREEN_VIEWPORT: ViewPort = ViewPort {
    origin: (0, 0),
    w: DISPLAY_WIDTH as i32,
    h: DISPLAY_HEIGHT as i32,
};
pub(super) static MODE5_VIEWPORT: ViewPort = ViewPort {
    origin: (0, 0),
    w: 160,
    h: 128,
};

pub(super) mod utils {
    use super::Point;

    #[inline]
    pub fn transform_bg_point(ref_point: Point, screen_x: i32, pa: i32, pc: i32) -> Point {
        let (ref_x, ref_y) = ref_point;
        ((ref_x + screen_x * pa) >> 8, (ref_y + screen_x * pc) >> 8)
    }
}
