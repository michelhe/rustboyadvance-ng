use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;

use js_sys::Float32Array;

use web_sys::AudioContext;
use web_sys::CanvasRenderingContext2d;

use rustboyadvance_core::keypad as gba_keypad;
use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::audio::AudioRingBuffer;

use bit::BitIndex;

#[wasm_bindgen]
pub struct Emulator {
    gba: GameBoyAdvance,
    interface: Rc<RefCell<Interface>>,
}

struct Interface {
    frame: Vec<u8>,
    sample_rate: i32,
    audio_ctx: AudioContext,
    audio_ring_buffer: AudioRingBuffer,
}

impl Drop for Interface {
    fn drop(&mut self) {
        let _ = self.audio_ctx.clone();
    }
}

impl Interface {
    fn new(audio_ctx: AudioContext) -> Result<Interface, JsValue> {
        Ok(Interface {
            frame: vec![0; 240 * 160 * 4],
            sample_rate: audio_ctx.sample_rate() as i32,
            audio_ctx: audio_ctx,
            audio_ring_buffer: Default::default(),
        })
    }
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

fn convert_sample(s: i16) -> f32 {
    (s as f32) / 32767_f32
}

impl AudioInterface for Interface {
    fn get_sample_rate(&self) -> i32 {
        self.sample_rate
    }

    fn push_sample(&mut self, samples: &[i16]) {
        let prod = self.audio_ring_buffer.producer();
        for s in samples.iter() {
            let _ = prod.push(*s);
        }
    }
}

#[wasm_bindgen]
impl Emulator {
    #[wasm_bindgen(constructor)]
    pub fn new(bios: &[u8], rom: &[u8]) -> Result<Emulator, JsValue> {
        let audio_ctx = web_sys::AudioContext::new()?;
        let interface = Rc::new(RefCell::new(Interface::new(audio_ctx)?));

        let gamepak = GamepakBuilder::new()
            .take_buffer(rom.to_vec().into_boxed_slice())
            .without_backup_to_file()
            .build()
            .unwrap();

        let gba = GameBoyAdvance::new(
            bios.to_vec().into_boxed_slice(),
            gamepak,
            interface.clone(),
            interface.clone(),
            interface.clone(),
        );

        Ok(Emulator { gba, interface })
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
        if let Some(key) = Emulator::map_key(event_key) {
            self.gba.get_key_state().set_bit(key as usize, false);
        }
    }

    pub fn key_up(&mut self, event_key: &str) {
        debug!("Key up: {}", event_key);
        if let Some(key) = Emulator::map_key(event_key) {
            self.gba.get_key_state().set_bit(key as usize, true);
        }
    }

    pub fn test_fps(&mut self) {
        use rustboyadvance_utils::FpsCounter;

        let mut fps_counter = FpsCounter::default();

        self.gba.skip_bios();
        for _ in 0..6000 {
            self.gba.frame();
            if let Some(fps) = fps_counter.tick() {
                info!("FPS: {}", fps);
            }
        }
    }

    pub fn collect_audio_samples(&self) -> Result<Float32Array, JsValue> {
        let mut interface = self.interface.borrow_mut();

        let consumer = interface.audio_ring_buffer.consumer();
        let mut samples = Vec::with_capacity(consumer.len());
        while let Some(sample) = consumer.pop() {
            samples.push(convert_sample(sample));
        }

        Ok(Float32Array::from(samples.as_slice()))
    }
}
