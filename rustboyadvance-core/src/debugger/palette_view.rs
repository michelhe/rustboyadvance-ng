// use sdl2::event::Event;
// use sdl2::pixels::Color;
// use sdl2::rect::{Point, Rect};
// use sdl2::render::Canvas;

// use crate::palette::{Palette, Rgb15};

// const PALETTE_RECT_WIDTH: u32 = 20;

// const SCREEN_WIDTH: u32 = 900;
// const SCREEN_HEIGHT: u32 = 500;

// struct ColoredRect {
//     index: usize,
//     rect: Rect,
//     color: Rgb15,
// }

// impl ColoredRect {
//     fn new(index: usize, x: i32, y: i32, c: Rgb15) -> ColoredRect {
//         ColoredRect {
//             index: index,
//             rect: Rect::new(x, y, PALETTE_RECT_WIDTH, PALETTE_RECT_WIDTH),
//             color: c,
//         }
//     }

//     fn draw(&self, canvas: &mut Canvas<sdl2::video::Window>) {
//         canvas.set_draw_color(Color::RGB(0, 0, 0));
//         canvas
//             .fill_rect(Rect::new(
//                 self.rect.x() - 1,
//                 self.rect.y() - 1,
//                 PALETTE_RECT_WIDTH + 2,
//                 PALETTE_RECT_WIDTH + 2,
//             ))
//             .unwrap();

//         let (r, g, b) = self.color.get_rgb24();
//         canvas.set_draw_color(Color::RGB(r, g, b));
//         canvas.fill_rect(self.rect).unwrap();
//     }
// }

// pub fn create_palette_view(palette_ram: &[u8]) {
//     let palette = Palette::from(palette_ram);

//     let sdl_context = sdl2::init().unwrap();
//     let video_subsystem = sdl_context.video().unwrap();

//     let window = video_subsystem
//         .window("PaletteView", SCREEN_WIDTH, SCREEN_HEIGHT)
//         .position_centered()
//         .build()
//         .unwrap();

//     let mut canvas = window.into_canvas().build().unwrap();

//     canvas.set_draw_color(Color::RGB(0xfa, 0xfa, 0xfa));
//     canvas.clear();

//     let mut bg_colors: Vec<ColoredRect> = Vec::with_capacity(256);
//     let mut fg_colors: Vec<ColoredRect> = Vec::with_capacity(256);

//     let initial_x = 30u32;
//     let mut y = 20u32;
//     let mut x = initial_x;
//     for i in 0..256 {
//         bg_colors.push(ColoredRect::new(
//             i,
//             x as i32,
//             y as i32,
//             palette.bg_colors[i],
//         ));
//         fg_colors.push(ColoredRect::new(
//             i,
//             x as i32 + 450,
//             y as i32,
//             palette.fg_colors[i],
//         ));

//         x = if (i + 1) % 16 == 0 {
//             y += 24;
//             initial_x
//         } else {
//             x + PALETTE_RECT_WIDTH + 4
//         }
//     }

//     for bgc in &bg_colors {
//         bgc.draw(&mut canvas);
//     }
//     for fgc in &fg_colors {
//         fgc.draw(&mut canvas);
//     }

//     canvas.present();

//     let mut event_pump = sdl_context.event_pump().unwrap();
//     'running: loop {
//         for event in event_pump.poll_iter() {
//             match event {
//                 Event::Quit { .. } => break 'running,
//                 Event::MouseButtonDown { x, y, .. } => {
//                     for bgc in &bg_colors {
//                         if bgc.rect.contains_point(Point::new(x, y)) {
//                             println!("BG Color #{}: {}", bgc.index, bgc.color);
//                         }
//                     }
//                     for fgc in &fg_colors {
//                         if fgc.rect.contains_point(Point::new(x, y)) {
//                             println!("FG Color #{}: {}", fgc.index, fgc.color);
//                         }
//                     }
//                 }
//                 _ => {}
//             }
//         }
//     }
// }
