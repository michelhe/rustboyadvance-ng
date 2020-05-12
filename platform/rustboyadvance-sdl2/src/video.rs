use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{Texture, TextureCreator, WindowCanvas};
use sdl2::video::WindowContext;

use rustboyadvance_core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use rustboyadvance_core::VideoInterface;

pub const SCREEN_WIDTH: u32 = DISPLAY_WIDTH as u32;
pub const SCREEN_HEIGHT: u32 = DISPLAY_HEIGHT as u32;

pub struct Sdl2Video<'a> {
    _tc: TextureCreator<WindowContext>, // only kept alive because of the texture
    texture: Texture<'a>,               // TODO - what happens if _tc is destroyed first ?
    canvas: WindowCanvas,
}

impl<'a> Sdl2Video<'a> {
    pub fn set_window_title(&mut self, title: &str) {
        self.canvas.window_mut().set_title(&title).unwrap();
    }
}

impl<'a> VideoInterface for Sdl2Video<'a> {
    fn render(&mut self, buffer: &[u32]) {
        self.texture
            .update(
                None,
                unsafe { std::mem::transmute::<&[u32], &[u8]>(buffer) },
                (SCREEN_WIDTH as usize) * 4,
            )
            .unwrap();
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
        self.canvas
            .copy(
                &self.texture,
                None,
                Some(Rect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT)),
            )
            .unwrap();
        self.canvas.present();
    }
}

pub fn create_video_interface<'a>(canvas: WindowCanvas) -> Sdl2Video<'a> {
    let mut tc = canvas.texture_creator();
    let texture = unsafe {
        let tc_ptr = &mut tc as *mut TextureCreator<WindowContext>;
        (*tc_ptr)
            .create_texture_streaming(PixelFormatEnum::BGRA32, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap()
    };
    Sdl2Video {
        _tc: tc,
        texture: texture,
        canvas: canvas,
    }
}
