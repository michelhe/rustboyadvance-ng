use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{TextureCreator, WindowCanvas};
use sdl2::video::WindowContext;
use sdl2::Sdl;

use rustboyadvance_ng::core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use rustboyadvance_ng::VideoInterface;

const SCREEN_WIDTH: u32 = DISPLAY_WIDTH as u32;
const SCREEN_HEIGHT: u32 = DISPLAY_HEIGHT as u32;
const SCALE: u32 = 3; // TODO control via CLI & support window resize

pub struct Sdl2Video {
    tc: TextureCreator<WindowContext>,
    canvas: WindowCanvas,
}

impl Sdl2Video {
    pub fn set_window_title(&mut self, title: &str) {
        self.canvas.window_mut().set_title(&title).unwrap();
    }
}

impl VideoInterface for Sdl2Video {
    fn render(&mut self, buffer: &[u32]) {
        let mut texture = self
            .tc
            .create_texture_streaming(PixelFormatEnum::BGRA32, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap();
        texture
            .update(
                None,
                unsafe { std::mem::transmute::<&[u32], &[u8]>(buffer) },
                (SCREEN_WIDTH as usize) * 4,
            )
            .unwrap();
        self.canvas
            .copy(
                &texture,
                None,
                Some(Rect::new(0, 0, SCREEN_WIDTH * SCALE, SCREEN_HEIGHT * SCALE)),
            )
            .unwrap();
        self.canvas.present();
    }
}

pub fn create_video_interface(sdl: &Sdl) -> Sdl2Video {
    let video_subsystem = sdl.video().unwrap();
    let window = video_subsystem
        .window(
            "RustBoyAdvance",
            SCREEN_WIDTH * SCALE,
            SCREEN_HEIGHT * SCALE,
        )
        .opengl()
        .position_centered()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().accelerated().build().unwrap();
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    let tc = canvas.texture_creator();
    Sdl2Video {
        tc: tc,
        canvas: canvas,
    }
}
