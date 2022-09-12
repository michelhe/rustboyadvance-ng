use rustboyadvance_utils::audio::SampleConsumer;
use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;

use js_sys::Float32Array;

use web_sys::CanvasRenderingContext2d;

use rustboyadvance_core::keypad as gba_keypad;
use rustboyadvance_core::prelude::*;

use bit::BitIndex;

#[wasm_bindgen]
pub struct Emulator {
    gba: GameBoyAdvance,
    audio_consumer: SampleConsumer,
    frame: Option<Box<[u8]>>,
}

fn translate_frame_to_u8(input_fb: &[u32], out_fb: &mut [u8]) {
    // TODO optimize
    for i in 0..input_fb.len() {
        let color = input_fb[i];
        out_fb[4 * i + 0] = ((color >> 16) & 0xff) as u8;
        out_fb[4 * i + 1] = ((color >> 8) & 0xff) as u8;
        out_fb[4 * i + 2] = (color & 0xff) as u8;
        out_fb[4 * i + 3] = 255;
    }
}

fn convert_sample(s: i16) -> f32 {
    (s as f32) / 32767_f32
}

#[wasm_bindgen]
impl Emulator {
    #[wasm_bindgen(constructor)]
    pub fn new(bios: &[u8], rom: &[u8]) -> Result<Emulator, JsValue> {
        let audio_ctx = web_sys::AudioContext::new()?;
        let (audio_device, audio_consumer) =
            SimpleAudioInterface::create_channel(audio_ctx.sample_rate() as i32, None);

        let gamepak = GamepakBuilder::new()
            .take_buffer(rom.to_vec().into_boxed_slice())
            .without_backup_to_file()
            .build()
            .unwrap();

        let gba = GameBoyAdvance::new(bios.to_vec().into_boxed_slice(), gamepak, audio_device);

        Ok(Emulator {
            gba,
            audio_consumer,
            frame: Some(vec![0; 240 * 160 * 4].into_boxed_slice()),
        })
    }

    pub fn skip_bios(&mut self) {
        self.gba.skip_bios();
    }

    pub fn run_frame(&mut self, ctx: &CanvasRenderingContext2d) -> Result<(), JsValue> {
        self.gba.frame();
        let mut frame = self.frame.take().unwrap();
        translate_frame_to_u8(self.gba.get_frame_buffer(), &mut frame);
        let data =
            web_sys::ImageData::new_with_u8_clamped_array_and_sh(Clamped(&mut frame), 240, 160)?;
        self.frame.replace(frame);
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
            self.gba.get_key_state_mut().set_bit(key as usize, false);
        }
    }

    pub fn key_up(&mut self, event_key: &str) {
        debug!("Key up: {}", event_key);
        if let Some(key) = Emulator::map_key(event_key) {
            self.gba.get_key_state_mut().set_bit(key as usize, true);
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

    pub fn collect_audio_samples(&mut self) -> Result<Float32Array, JsValue> {
        let mut samples = Vec::with_capacity(self.audio_consumer.len());
        while let Some(sample) = self.audio_consumer.pop() {
            samples.push(convert_sample(sample));
        }

        Ok(Float32Array::from(samples.as_slice()))
    }
}
