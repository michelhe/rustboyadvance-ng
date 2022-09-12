use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::audio::SampleConsumer;
// use rustboyadvance_core::util::FpsCounter;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use jni::objects::{GlobalRef, JMethodID, JObject, JString, JValue};
use jni::signature;
use jni::sys::{jboolean, jbyteArray, jintArray, jmethodID};
use jni::JNIEnv;

use crate::audio::{self, connector::AudioJNIConnector, thread::AudioThreadCommand};

struct Renderer {
    renderer_ref: GlobalRef,
    frame_buffer_ref: GlobalRef,
    mid_render_frame: jmethodID,
}

impl Renderer {
    fn new(env: &JNIEnv, renderer_obj: JObject) -> Result<Renderer, String> {
        let renderer_ref = env
            .new_global_ref(renderer_obj)
            .map_err(|e| format!("failed to add new global ref, error: {:?}", e))?;

        let frame_buffer = env
            .new_int_array(240 * 160)
            .map_err(|e| format!("failed to create framebuffer, error: {:?}", e))?;
        let frame_buffer_ref = env
            .new_global_ref(frame_buffer)
            .map_err(|e| format!("failed to add new global ref, error: {:?}", e))?;
        let renderer_klass = env
            .get_object_class(renderer_ref.as_obj())
            .expect("failed to get renderer class");
        let mid_render_frame = env
            .get_method_id(renderer_klass, "renderFrame", "([I)V")
            .expect("failed to get methodID for renderFrame")
            .into_inner();

        Ok(Renderer {
            renderer_ref,
            frame_buffer_ref,
            mid_render_frame,
        })
    }

    #[inline]
    fn render_frame(&self, env: &JNIEnv, buffer: &[u32]) {
        unsafe {
            env.set_int_array_region(
                self.frame_buffer_ref.as_obj().into_inner(),
                0,
                std::mem::transmute::<&[u32], &[i32]>(buffer),
            )
            .unwrap();
        }

        env.call_method_unchecked(
            self.renderer_ref.as_obj(),
            JMethodID::from(self.mid_render_frame),
            signature::JavaType::Primitive(signature::Primitive::Void),
            &[JValue::from(self.frame_buffer_ref.as_obj())],
        )
        .expect("failed to call renderFrame");
    }
}

struct Keypad {
    keypad_ref: GlobalRef,
    mid_get_key_state: jmethodID,
}

impl Keypad {
    fn new(env: &JNIEnv, keypad_obj: JObject) -> Keypad {
        let keypad_ref = env
            .new_global_ref(keypad_obj)
            .expect("failed to create keypad_ref");
        let keypad_klass = env
            .get_object_class(keypad_ref.as_obj())
            .expect("failed to create keypad class");
        let mid_get_key_state = env
            .get_method_id(keypad_klass, "getKeyState", "()I")
            .expect("failed to get methodID for getKeyState")
            .into_inner();

        Keypad {
            keypad_ref,
            mid_get_key_state,
        }
    }

    #[inline]
    fn get_key_state(&self, env: &JNIEnv) -> u16 {
        match env.call_method_unchecked(
            self.keypad_ref.as_obj(),
            JMethodID::from(self.mid_get_key_state),
            signature::JavaType::Primitive(signature::Primitive::Int),
            &[],
        ) {
            Ok(JValue::Int(key_state)) => key_state as u16,
            _ => panic!("failed to call getKeyState"),
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum EmulationState {
    Initial,
    Pausing,
    Paused,
    Running(bool),
    Stopping,
    Stopped,
}

impl Default for EmulationState {
    fn default() -> EmulationState {
        EmulationState::Initial
    }
}

fn create_audio(
    env: &JNIEnv,
    audio_player_obj: JObject,
) -> Result<(Box<SimpleAudioInterface>, SampleConsumer), String> {
    let sample_rate = audio::util::get_sample_rate(env, audio_player_obj)?;
    let sample_count = audio::util::get_sample_count(env, audio_player_obj)? as usize;
    Ok(SimpleAudioInterface::create_channel(
        sample_rate,
        Some(sample_count * 2),
    ))
}

pub struct EmulatorContext {
    audio_consumer: Option<SampleConsumer>,
    renderer: Renderer,
    audio_player_ref: GlobalRef,
    keypad: Keypad,
    pub emustate: Mutex<EmulationState>,
    pub gba: GameBoyAdvance,
}

impl EmulatorContext {
    pub fn native_open_context(
        env: &JNIEnv,
        bios: jbyteArray,
        rom: jbyteArray,
        renderer_obj: JObject,
        audio_player: JObject,
        keypad_obj: JObject,
        save_file: JString,
        skip_bios: jboolean,
    ) -> Result<EmulatorContext, String> {
        let bios = env
            .convert_byte_array(bios)
            .map_err(|e| format!("could not get bios buffer, error {}", e))?
            .into_boxed_slice();
        let rom = env
            .convert_byte_array(rom)
            .map_err(|e| format!("could not get rom buffer, error {}", e))?
            .into_boxed_slice();
        let save_file: String = env
            .get_string(save_file)
            .map_err(|_| String::from("could not get save path"))?
            .into();
        let gamepak = GamepakBuilder::new()
            .take_buffer(rom)
            .save_path(&Path::new(&save_file))
            .build()
            .map_err(|e| format!("failed to load rom, gba result: {:?}", e))?;
        info!("Loaded ROM file {:?}", gamepak.header);

        info!("Creating renderer");
        let renderer = Renderer::new(env, renderer_obj)?;

        info!("Creating GBA Instance");
        let audio_player_ref = env.new_global_ref(audio_player).unwrap();
        let (audio_device, audio_consumer) = create_audio(env, audio_player_ref.as_obj())?;
        let mut gba = GameBoyAdvance::new(bios, gamepak, audio_device);
        if skip_bios != 0 {
            info!("skipping bios");
            gba.skip_bios();
        }

        info!("creating keypad");
        let keypad = Keypad::new(env, keypad_obj);

        info!("creating context");
        let context = EmulatorContext {
            gba,
            keypad,
            renderer,
            audio_player_ref,
            emustate: Mutex::new(EmulationState::default()),
            audio_consumer: Some(audio_consumer),
        };
        Ok(context)
    }

    pub fn native_open_saved_state(
        env: &JNIEnv,
        bios: jbyteArray,
        rom: jbyteArray,
        savestate: jbyteArray,
        renderer_obj: JObject,
        audio_player: JObject,
        keypad_obj: JObject,
    ) -> Result<EmulatorContext, String> {
        let bios = env
            .convert_byte_array(bios)
            .map_err(|e| format!("could not get bios buffer, error {}", e))?
            .into_boxed_slice();
        let rom = env
            .convert_byte_array(rom)
            .map_err(|e| format!("could not get rom buffer, error {}", e))?
            .into_boxed_slice();
        let savestate = env
            .convert_byte_array(savestate)
            .map_err(|e| format!("could not get savestate buffer, error {}", e))?;

        let renderer = Renderer::new(env, renderer_obj)?;
        let audio_player_ref = env.new_global_ref(audio_player).unwrap();
        let (audio_device, audio_consumer) = create_audio(env, audio_player_ref.as_obj())?;
        let gba =
            GameBoyAdvance::from_saved_state(&savestate, bios, rom, audio_device).map_err(|e| {
                format!(
                    "failed to create GameBoyAdvance from saved savestate, error {:?}",
                    e
                )
            })?;

        let keypad = Keypad::new(env, keypad_obj);

        Ok(EmulatorContext {
            gba,
            keypad,
            renderer,
            audio_player_ref,
            emustate: Mutex::new(EmulationState::default()),
            audio_consumer: Some(audio_consumer),
        })
    }

    fn render_video(&mut self, env: &JNIEnv) {
        self.renderer.render_frame(env, self.gba.get_frame_buffer());
    }

    /// Lock the emulation loop in order to perform updates to the struct
    pub fn lock_and_get_gba(&mut self) -> (MutexGuard<EmulationState>, &mut GameBoyAdvance) {
        (self.emustate.lock().unwrap(), &mut self.gba)
    }

    /// Run the emulation main loop
    pub fn native_run(&mut self, env: &JNIEnv) -> Result<(), jni::errors::Error> {
        const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000u64 / 60);

        // Set the state to running
        *self.emustate.lock().unwrap() = EmulationState::Running(false);

        // Extract current JVM
        let jvm = env.get_java_vm().unwrap();

        // Instanciate an audio player connector
        let audio_connector = AudioJNIConnector::new(env, self.audio_player_ref.as_obj());

        // Spawn the audio worker thread, give it the audio connector, jvm and ringbuffer consumer
        // Note - after this operation `self` no longer has `audio_consumer`
        let (audio_thread_handle, audio_thread_tx) = audio::thread::spawn_audio_worker_thread(
            audio_connector,
            jvm,
            self.audio_consumer.take().unwrap(),
        );

        info!("starting main emulation loop");

        // let mut fps_counter = FpsCounter::default();

        'running: loop {
            let emustate = *self.emustate.lock().unwrap();

            let vsync = match emustate {
                EmulationState::Initial => unsafe { std::hint::unreachable_unchecked() },
                EmulationState::Stopped => unsafe { std::hint::unreachable_unchecked() },
                EmulationState::Pausing => {
                    info!("emulation pause requested");
                    *self.emustate.lock().unwrap() = EmulationState::Paused;
                    continue;
                }
                EmulationState::Paused => continue,
                EmulationState::Stopping => break 'running,
                EmulationState::Running(turbo) => !turbo,
            };

            let start_time = Instant::now();
            // check key state
            *self.gba.get_key_state_mut() = self.keypad.get_key_state(env);

            // run frame
            self.gba.frame();

            // render video
            self.render_video(env);

            // request audio worker to render the audio now
            audio_thread_tx
                .send(AudioThreadCommand::RenderAudio)
                .unwrap();

            // if let Some(fps) = fps_counter.tick() {
            //     info!("FPS {}", fps);
            // }

            if vsync {
                let time_passed = start_time.elapsed();
                let delay = FRAME_TIME.checked_sub(time_passed);
                match delay {
                    None => {}
                    Some(delay) => {
                        std::thread::sleep(delay);
                    }
                }
            }
        }

        info!("stopping, terminating audio worker");
        audio_thread_tx.send(AudioThreadCommand::Terminate).unwrap(); // we surely have an endpoint, so it will work
        info!("waiting for audio worker to complete");

        let (audio_connector, audio_consumer) = audio_thread_handle.join().unwrap();
        self.audio_consumer.replace(audio_consumer);
        info!("audio worker terminated");

        audio_connector.pause(env);

        *self.emustate.lock().unwrap() = EmulationState::Stopped;

        Ok(())
    }

    pub fn native_get_framebuffer(&mut self, env: &JNIEnv) -> jintArray {
        let fb = env.new_int_array(240 * 160).unwrap();
        self.pause();
        unsafe {
            env.set_int_array_region(
                fb,
                0,
                std::mem::transmute::<&[u32], &[i32]>(self.gba.get_frame_buffer()),
            )
            .unwrap();
        }
        self.resume();

        fb
    }

    pub fn pause(&mut self) {
        *self.emustate.lock().unwrap() = EmulationState::Pausing;
        while *self.emustate.lock().unwrap() != EmulationState::Paused {
            info!("awaiting pause...")
        }
    }

    pub fn resume(&mut self) {
        *self.emustate.lock().unwrap() = EmulationState::Running(false);
    }

    pub fn set_turbo(&mut self, turbo: bool) {
        *self.emustate.lock().unwrap() = EmulationState::Running(turbo);
    }

    pub fn request_stop(&mut self) {
        if EmulationState::Stopped != *self.emustate.lock().unwrap() {
            *self.emustate.lock().unwrap() = EmulationState::Stopping;
        }
    }

    pub fn is_stopped(&self) -> bool {
        *self.emustate.lock().unwrap() == EmulationState::Stopped
    }
}
