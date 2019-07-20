use std::time::Duration;

use sdl2::event::Event;
use sdl2::pixels::Color;
use sdl2::rect::Point;

use crate::core::gba::GameBoyAdvance;
use crate::core::gpu::Gpu;

const SCREEN_WIDTH: u32 = Gpu::DISPLAY_WIDTH as u32;
const SCREEN_HEIGHT: u32 = Gpu::DISPLAY_HEIGHT as u32;

pub fn create_render_view(gba: &GameBoyAdvance) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem
        .window("RenderView", SCREEN_WIDTH, SCREEN_HEIGHT)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();

    let mut event_pump = sdl_context.event_pump().unwrap();
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::MouseButtonDown { x, y, .. } => {
                    println!("({},{}) {:x}", x, y, x + y * (Gpu::DISPLAY_WIDTH as i32));
                }
                _ => {}
            }
        }

        canvas.set_draw_color(Color::RGB(0xfa, 0xfa, 0xfa));
        canvas.clear();

        for y in 0..Gpu::DISPLAY_HEIGHT {
            for x in 0..Gpu::DISPLAY_WIDTH {
                let index = (x as usize) + (y as usize) * (512 as usize);
                let color = gba.gpu.pixeldata[index];
                let rgb24: Color = color.into();
                canvas.set_draw_color(rgb24);
                canvas.draw_point(Point::from((x as i32, y as i32)));
            }
        }

        canvas.present();

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}
