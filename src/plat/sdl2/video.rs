use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{Point, Rect};
use sdl2::render::{TextureCreator, WindowCanvas};
use sdl2::video::WindowContext;
use sdl2::Sdl;

use rustboyadvance_ng::core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use rustboyadvance_ng::util::FpsCounter;
use rustboyadvance_ng::VideoInterface;

const SCREEN_WIDTH: u32 = DISPLAY_WIDTH as u32;
const SCREEN_HEIGHT: u32 = DISPLAY_HEIGHT as u32;
const SCALE: u32 = 3; // TODO control via CLI & support window resize

pub struct PlatformSdl2_VideoInterface {
    tc: TextureCreator<WindowContext>,
    canvas: WindowCanvas,
    fps_counter: FpsCounter,
}

impl PlatformSdl2_VideoInterface {
    pub fn set_window_title(&mut self, title: &str) {
        self.canvas.window_mut().set_title(&title);
    }
}

impl VideoInterface for PlatformSdl2_VideoInterface {
    fn render(&mut self, buffer: &[u32]) {
        let mut texture = self
            .tc
            .create_texture_target(PixelFormatEnum::RGB24, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap();
        self.canvas
            .with_texture_canvas(&mut texture, |texture_canvas| {
                for y in 0i32..(SCREEN_HEIGHT as i32) {
                    for x in 0i32..(SCREEN_WIDTH as i32) {
                        let c = buffer[index2d!(x, y, SCREEN_WIDTH as i32) as usize];
                        let color = Color::RGB((c >> 16) as u8, (c >> 8) as u8, c as u8);
                        texture_canvas.set_draw_color(color);
                        let _ = texture_canvas.draw_point(Point::from((x, y)));
                    }
                }
            })
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

pub fn create_video_interface(sdl: &Sdl) -> PlatformSdl2_VideoInterface {
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
    PlatformSdl2_VideoInterface {
        tc: tc,
        canvas: canvas,
        fps_counter: Default::default(),
    }
}
