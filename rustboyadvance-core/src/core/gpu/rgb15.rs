//! Helper type to deal with the GBA's 15bit color

use serde::{Deserialize, Serialize};

bitfield! {
    #[repr(transparent)]
    #[derive(Serialize, Deserialize, Copy, Clone, Default, PartialEq)]
    pub struct Rgb15(u16);
    impl Debug;
    pub r, set_r: 4, 0;
    pub g, set_g: 9, 5;
    pub b, set_b: 14, 10;
}

impl Rgb15 {
    pub const BLACK: Rgb15 = Rgb15(0);
    pub const WHITE: Rgb15 = Rgb15(0x7fff);
    pub const TRANSPARENT: Rgb15 = Rgb15(0x8000);

    pub fn to_rgb24(&self) -> u32 {
        ((self.r() as u32) << 19) | ((self.g() as u32) << 11) | ((self.b() as u32) << 3)
    }

    pub fn from_rgb(r: u16, g: u16, b: u16) -> Rgb15 {
        let mut c = Rgb15(0);
        c.set_r(r);
        c.set_g(g);
        c.set_b(b);
        c
    }

    pub fn get_rgb(&self) -> (u16, u16, u16) {
        (self.r(), self.g(), self.b())
    }

    pub fn is_transparent(&self) -> bool {
        self.0 == 0x8000
    }
}
