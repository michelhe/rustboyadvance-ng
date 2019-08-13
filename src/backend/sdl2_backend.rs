use std::time;

use crate::bit::BitIndex;

extern crate sdl2;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{Point, Rect};
use sdl2::render::{TextureCreator, WindowCanvas};
use sdl2::video::WindowContext;

use super::EmulatorBackend;
use crate::core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::core::keypad;

pub struct Sdl2Backend {
    event_pump: sdl2::EventPump,
    tc: TextureCreator<WindowContext>,
    canvas: WindowCanvas,
    frames_rendered: u32,
    fps_timer: time::Instant,
    keyinput: u16,
}

const SCREEN_WIDTH: u32 = DISPLAY_WIDTH as u32;
const SCREEN_HEIGHT: u32 = DISPLAY_HEIGHT as u32;

impl Sdl2Backend {
    pub fn new() -> Sdl2Backend {
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem
            .window("RustBoyAdvance", SCREEN_WIDTH, SCREEN_HEIGHT)
            .opengl()
            .position_centered()
            .build()
            .unwrap();

        let mut canvas = window.into_canvas().accelerated().build().unwrap();
        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        let tc = canvas.texture_creator();
        let event_pump = sdl_context.event_pump().unwrap();

        Sdl2Backend {
            canvas: canvas,
            event_pump: event_pump,
            tc: tc,
            frames_rendered: 0,
            fps_timer: time::Instant::now(),
            keyinput: keypad::KEYINPUT_ALL_RELEASED,
        }
    }
}

impl EmulatorBackend for Sdl2Backend {
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
                Some(Rect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT)),
            )
            .unwrap();
        self.canvas.present();

        self.frames_rendered += 1;
        if self.fps_timer.elapsed() >= time::Duration::from_secs(1) {
            self.fps_timer = time::Instant::now();
            let title = format!("rustboyadvance-ng ({} fps)", self.frames_rendered);
            self.canvas.window_mut().set_title(&title);
            self.frames_rendered = 0;
        }
    }

    fn get_key_state(&mut self) -> u16 {
        for event in self.event_pump.poll_iter() {
            match event {
                Event::KeyDown {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_keypad(keycode) {
                        self.keyinput.set_bit(key as usize, false);
                    }
                }
                Event::KeyUp {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(key) = keycode_to_keypad(keycode) {
                        self.keyinput.set_bit(key as usize, true);
                    }
                }
                Event::Quit { .. } => panic!("quit!"),
                _ => {}
            }
        }

        self.keyinput
    }
}

fn keycode_to_keypad(keycode: Keycode) -> Option<keypad::Keys> {
    match keycode {
        Keycode::Up => Some(keypad::Keys::Up),
        Keycode::Down => Some(keypad::Keys::Down),
        Keycode::Left => Some(keypad::Keys::Left),
        Keycode::Right => Some(keypad::Keys::Right),
        _ => None,
    }
}
