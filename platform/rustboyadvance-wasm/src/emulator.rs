use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;
use web_sys::CanvasRenderingContext2d;

use rustboyadvance_core::core::keypad as gba_keypad;
use rustboyadvance_core::prelude::*;

use bit::BitIndex;

#[wasm_bindgen]
pub struct Emulator {
    gba: GameBoyAdvance,
    interface: Rc<RefCell<Interface>>,
}

struct Interface {
    frame: Vec<u8>,
    keyinput: u16,
}

impl VideoInterface for Interface {
    fn render(&mut self, buffer: &[u32]) {
        // TODO optimize
        for i in 0..buffer.len() {
            let color = buffer[i];
            self.frame[4 * i + 0] = ((color >> 16) & 0xff) as u8;
            self.frame[4 * i + 1] = ((color >> 8) & 0xff) as u8;
            self.frame[4 * i + 2] = (color & 0xff) as u8;
            self.frame[4 * i + 3] = 255;
        }
    }
}

impl AudioInterface for Interface {}

impl InputInterface for Interface {
    fn poll(&mut self) -> u16 {
        self.keyinput
    }
}

#[wasm_bindgen]
impl Emulator {
    #[wasm_bindgen(constructor)]
    pub fn new(bios: &[u8], rom: &[u8]) -> Emulator {
        let gamepak = GamepakBuilder::new()
            .take_buffer(rom.to_vec().into_boxed_slice())
            .without_backup_to_file()
            .build()
            .unwrap();

        let interface = Rc::new(RefCell::new(Interface {
            frame: vec![0; 240 * 160 * 4],
            keyinput: gba_keypad::KEYINPUT_ALL_RELEASED,
        }));

        let gba = GameBoyAdvance::new(
            bios.to_vec().into_boxed_slice(),
            gamepak,
            interface.clone(),
            interface.clone(),
            interface.clone(),
        );

        Emulator { gba, interface }
    }

    pub fn skip_bios(&mut self) {
        self.gba.skip_bios();
    }

    pub fn run_frame(&mut self, ctx: &CanvasRenderingContext2d) -> Result<(), JsValue> {
        self.gba.frame();
        let mut frame_buffer = &mut self.interface.borrow_mut().frame;
        let data = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&mut frame_buffer),
            240,
            160,
        )
        .unwrap();
        ctx.put_image_data(&data, 0.0, 0.0)
    }

    fn map_key(event_key: &str) -> Option<gba_keypad::Keys> {
        match event_key {
            "Enter" => Some(gba_keypad::Keys::Start),
            "Backspace" => Some(gba_keypad::Keys::Select),
            "ArrowUp" => Some(gba_keypad::Keys::Up),
            "ArrowDown" => Some(gba_keypad::Keys::Down),
            "ArrowLeft" => Some(gba_keypad::Keys::Left),
            "ArrowRight" => Some(gba_keypad::Keys::Right),
            "z" => Some(gba_keypad::Keys::ButtonB),
            "x" => Some(gba_keypad::Keys::ButtonA),
            "a" => Some(gba_keypad::Keys::ButtonL),
            "s" => Some(gba_keypad::Keys::ButtonR),
            _ => None,
        }
    }

    pub fn key_down(&mut self, event_key: &str) {
        debug!("Key down: {}", event_key);
        let mut interface = self.interface.borrow_mut();
        if let Some(key) = Emulator::map_key(event_key) {
            interface.keyinput.set_bit(key as usize, false);
        }
    }

    pub fn key_up(&mut self, event_key: &str) {
        debug!("Key up: {}", event_key);
        let mut interface = self.interface.borrow_mut();
        if let Some(key) = Emulator::map_key(event_key) {
            interface.keyinput.set_bit(key as usize, true);
        }
    }

    pub fn test_fps(&mut self) {
        use rustboyadvance_core::util::FpsCounter;

        let mut fps_counter = FpsCounter::default();

        self.gba.skip_bios();
        for _ in 0..6000 {
            self.gba.frame();
            if let Some(fps) = fps_counter.tick() {
                info!("FPS: {}", fps);
            }
        }
    }
}
