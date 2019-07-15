use std::fmt;

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

#[derive(Debug, Copy, Clone, Default)]
pub struct Rgb15 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<u16> for Rgb15 {
    fn from(v: u16) -> Rgb15 {
        use bit::BitIndex;
        Rgb15 {
            r: v.bit_range(0..5) as u8,
            g: v.bit_range(5..10) as u8,
            b: v.bit_range(10..15) as u8,
        }
    }
}

impl fmt::Display for Rgb15 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Rgb15({:#x},{:#x},{:#x})", self.r, self.g, self.b)
    }
}

impl Rgb15 {
    /// Convert 15-bit high color to a 24-bit true color.
    pub fn get_rgb24(&self) -> (u8, u8, u8) {
        (self.r << 3, self.g << 3, self.b << 3)
    }
}

#[derive(Debug, Primitive, Copy, Clone)]
pub enum PixelFormat {
    BPP4 = 0,
    BPP8 = 1,
}

pub struct Palette {
    pub bg_colors: [Rgb15; 256],
    pub fg_colors: [Rgb15; 256],
}

impl From<&[u8]> for Palette {
    fn from(bytes: &[u8]) -> Palette {
        let mut rdr = Cursor::new(bytes);

        let mut bg_colors: [Rgb15; 256] = [0.into(); 256];
        for i in 0..256 {
            bg_colors[i] = rdr.read_u16::<LittleEndian>().unwrap().into();
        }
        let mut fg_colors: [Rgb15; 256] = [0.into(); 256];
        for i in 0..256 {
            fg_colors[i] = rdr.read_u16::<LittleEndian>().unwrap().into();
        }
        Palette {
            bg_colors,
            fg_colors,
        }
    }
}
