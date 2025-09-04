use rustboyadvance_core::gpu::{DISPLAY_HEIGHT, DISPLAY_WIDTH};

use crossterm::{ExecutableCommand, terminal};

pub const SCREEN_WIDTH: u32 = DISPLAY_WIDTH as u32;
pub const SCREEN_HEIGHT: u32 = DISPLAY_HEIGHT as u32;

pub struct Renderer {}

pub fn init<'a>() -> Renderer {
    // force pre-computing of kitty support, since the event handler breaks the function, and the result is cached
    let _ = viuer::get_kitty_support();

    Renderer {}
}

impl Renderer {
    pub fn clear(&mut self, stdout: &mut std::io::Stdout) {
        stdout
            .execute(terminal::Clear(terminal::ClearType::All))
            .expect("failed to clear screen");
    }

    pub fn render(&mut self, buffer: &[u32], information: &Option<String>) {
        let v = buffer
            .iter()
            .flat_map(|px| {
                [
                    ((px >> 16) & 0xff) as u8,
                    ((px >> 8) & 0xff) as u8,
                    (px & 0xff) as u8,
                ]
            })
            .collect::<Vec<u8>>();

        let img = image::RgbImage::from_raw(SCREEN_WIDTH, SCREEN_HEIGHT, v)
            .unwrap()
            .into();
        let config = viuer::Config {
            ..Default::default()
        };

        viuer::print(&img, &config).unwrap();
        if let Some(information) = information {
            print!("\r\n{}", information);
        }
    }
}
