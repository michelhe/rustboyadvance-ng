use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::FpsCounter;
use rustboyadvance_utils::audio::SampleConsumer;

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use jni::objects::{GlobalRef, JByteArray, JIntArray, JMethodID, JObject, JString, JValue};
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
    fn new(env: &mut JNIEnv, renderer_obj: JObject) -> Result<Renderer, String> {
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
            .into_raw();

        Ok(Renderer {
            renderer_ref,
            frame_buffer_ref,
            mid_render_frame,
        })
    }

    #[inline]
    fn render_frame(&self, env: &mut JNIEnv, buffer: &[u32]) {
        unsafe {
            let fbo: &JIntArray = self.frame_buffer_ref.as_obj().into();
            env.set_int_array_region(
                fbo,
                0,
                std::mem::transmute::<&[u32], &[i32]>(buffer),
            )
            .unwrap();
        }

        unsafe { env.call_method_unchecked(
            self.renderer_ref.as_obj(),
            JMethodID::from_raw(self.mid_render_frame),
            signature::ReturnType::Primitive(signature::Primitive::Void),
            &[JValue::from(self.frame_buffer_ref.as_obj()).as_jni()],
        )
        .expect("failed to call renderFrame") };
    }
}

struct Keypad {
    keypad_ref: GlobalRef,
    mid_get_key_state: jmethodID,
}

impl Keypad {
    fn new(env: &mut JNIEnv, keypad_obj: JObject) -> Keypad {
        let keypad_ref = env
            .new_global_ref(keypad_obj)
            .expect("failed to create keypad_ref");
        let keypad_klass = env
            .get_object_class(keypad_ref.as_obj())
            .expect("failed to create keypad class");
        let mid_get_key_state = env
            .get_method_id(keypad_klass, "getKeyState", "()I")
            .expect("failed to get methodID for getKeyState")
            .into_raw();

        Keypad {
            keypad_ref,
            mid_get_key_state,
        }
    }

    #[inline]
    fn get_key_state(&self, env: &mut JNIEnv) -> u16 {
        unsafe { 
            match env.call_method_unchecked(
                self.keypad_ref.as_obj(),
                JMethodID::from_raw(self.mid_get_key_state),
                signature::ReturnType::Primitive(signature::Primitive::Int),
                &[],
            ) {
                Ok(result) => match result.i() {
                    Ok(i) => i as u16,
                    _ => panic!("failed to call getKeyState"),
                },
                Err(_) => panic!("failed to call getKeyState"),
            }
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
    env: &mut JNIEnv,
    audio_player_obj: &JObject,
) -> Result<(Box<SimpleAudioInterface>, SampleConsumer), String> {
    let sample_rate = audio::util::get_sample_rate(env, audio_player_obj)?;
    let sample_count = audio::util::get_sample_count(env, audio_player_obj)? as usize;
    Ok(SimpleAudioInterface::create_channel(
        sample_rate,
        Some(sample_count * 2),
    ))
}

/// RBAREC01 file format: magic + (cycle: u64 LE, state: u16 LE) records.
/// Same format the SDL frontend emits and fps_bench reads.
const REC_MAGIC: &[u8; 8] = b"RBAREC01";

/// Appends keypad edges to a file as the emulator sees them. Only records
/// when the state actually changed vs the previous sample so the file stays
/// small.
struct Recorder {
    w: BufWriter<File>,
    last_state: u16,
}

impl Recorder {
    fn open(path: &str) -> std::io::Result<Self> {
        let f = File::create(path)?;
        let mut w = BufWriter::new(f);
        w.write_all(REC_MAGIC)?;
        Ok(Recorder { w, last_state: 0 })
    }

    /// Called once per frame with the current keypad state and the
    /// emulator's cycle counter. Writes a record only on an edge.
    fn observe(&mut self, cycle: u64, state: u16) -> std::io::Result<()> {
        if state == self.last_state {
            return Ok(());
        }
        self.w.write_all(&cycle.to_le_bytes())?;
        self.w.write_all(&state.to_le_bytes())?;
        self.last_state = state;
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

/// Plays back a recording against the emulator. `apply_due` returns the
/// state the emulator should use for the upcoming frame, overriding
/// whatever the Java keypad says.
struct Replayer {
    events: Vec<(u64, u16)>, // (cycle, state)
    cursor: usize,
    current_state: u16,
}

impl Replayer {
    fn load(path: &str) -> std::io::Result<Self> {
        let mut f = File::open(path)?;
        let mut magic = [0u8; 8];
        f.read_exact(&mut magic)?;
        if &magic != REC_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "recording magic mismatch",
            ));
        }
        let mut events = Vec::new();
        let mut buf = [0u8; 10];
        loop {
            match f.read_exact(&mut buf) {
                Ok(()) => {
                    let c = u64::from_le_bytes(buf[0..8].try_into().unwrap());
                    let s = u16::from_le_bytes(buf[8..10].try_into().unwrap());
                    events.push((c, s));
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }
        Ok(Replayer { events, cursor: 0, current_state: 0 })
    }

    /// Advance through events whose cycle has already passed, letting
    /// later events win. Returns the state to hand to the emulator.
    fn apply_due(&mut self, now: u64) -> u16 {
        while self.cursor < self.events.len() && self.events[self.cursor].0 <= now {
            self.current_state = self.events[self.cursor].1;
            self.cursor += 1;
        }
        self.current_state
    }

    fn exhausted(&self) -> bool {
        self.cursor >= self.events.len()
    }
}

pub struct EmulatorContext {
    audio_consumer: Option<SampleConsumer>,
    renderer: Renderer,
    audio_player_ref: GlobalRef,
    keypad: Keypad,
    pub emustate: Mutex<EmulationState>,
    pub gba: GameBoyAdvance,
    /// On-device keypad recorder. When Some, every frame writes an edge
    /// record to the backing file. None means no recording.
    recorder: Mutex<Option<Recorder>>,
    /// On-device keypad replayer. When Some, every frame overrides the
    /// keypad state with the replayer's next event.
    replayer: Mutex<Option<Replayer>>,
}

impl EmulatorContext {
    pub fn native_open_context(
        env: &mut JNIEnv,
        bios: jbyteArray,
        rom: jbyteArray,
        renderer_obj: JObject,
        audio_player: JObject,
        keypad_obj: JObject,
        save_file: JString,
        skip_bios: jboolean,
    ) -> Result<EmulatorContext, String> {
        let bios = env
            .convert_byte_array(unsafe { JByteArray::from_raw(bios) })
            .map_err(|e| format!("could not get bios buffer, error {}", e))?
            .into_boxed_slice();
        let rom = env
            .convert_byte_array(unsafe { JByteArray::from_raw(rom) })
            .map_err(|e| format!("could not get rom buffer, error {}", e))?
            .into_boxed_slice();
        let save_file: String = env
            .get_string(&save_file)
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
            recorder: Mutex::new(None),
            replayer: Mutex::new(None),
        };
        Ok(context)
    }

    pub fn native_open_saved_state(
        env: &mut JNIEnv,
        bios: jbyteArray,
        rom: jbyteArray,
        savestate: jbyteArray,
        renderer_obj: JObject,
        audio_player: JObject,
        keypad_obj: JObject,
    ) -> Result<EmulatorContext, String> {
        let bios = env
            .convert_byte_array(unsafe { JByteArray::from_raw(bios) })
            .map_err(|e| format!("could not get bios buffer, error {}", e))?
            .into_boxed_slice();
        let rom = env
            .convert_byte_array(unsafe { JByteArray::from_raw(rom) })
            .map_err(|e| format!("could not get rom buffer, error {}", e))?
            .into_boxed_slice();
        let savestate = env
            .convert_byte_array(unsafe { JByteArray::from_raw(savestate) })
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
            recorder: Mutex::new(None),
            replayer: Mutex::new(None),
        })
    }

    fn render_video(&mut self, env: &mut JNIEnv) {
        self.renderer.render_frame(env, self.gba.get_frame_buffer());
    }

    /// Lock the emulation loop in order to perform updates to the struct
    pub fn lock_and_get_gba(&mut self) -> (MutexGuard<EmulationState>, &mut GameBoyAdvance) {
        (self.emustate.lock().unwrap(), &mut self.gba)
    }

    /// Run the emulation main loop
    pub fn native_run(&mut self, env: &mut JNIEnv) -> Result<(), jni::errors::Error> {
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

        let mut fps_counter = FpsCounter::default();

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
            // check key state: live from the Java keypad unless a replay
            // session is active, in which case the replay file overrides.
            let live_state = self.keypad.get_key_state(env);
            let gba_cycles = self.gba.cycles() as u64;
            let effective_state = {
                let mut replayer = self.replayer.lock().unwrap();
                if let Some(r) = replayer.as_mut() {
                    r.apply_due(gba_cycles)
                } else {
                    live_state
                }
            };
            *self.gba.get_key_state_mut() = effective_state;
            // If recording, log any edge the emulator saw this frame.
            if let Some(rec) = self.recorder.lock().unwrap().as_mut() {
                let _ = rec.observe(gba_cycles, effective_state);
            }

            // run frame
            self.gba.frame();

            // render video
            self.render_video(env);

            // request audio worker to render the audio now
            audio_thread_tx
                .send(AudioThreadCommand::RenderAudio)
                .unwrap();

            // Emit one log line per second with the measured FPS. Use the
            // LCD tag so `adb logcat -s RustdroidFps` filters cleanly.
            if let Some(fps) = fps_counter.tick() {
                info!(target: "RustdroidFps", "FPS {}", fps);
            }

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
                &fb,
                0,
                std::mem::transmute::<&[u32], &[i32]>(self.gba.get_frame_buffer()),
            )
            .unwrap();
        }
        self.resume();

        **fb
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

    /// Begin recording keypad edges to the file at `path`. Overwrites any
    /// existing recording. Stops and discards an active replay session.
    pub fn start_recording(&self, path: &str) -> Result<(), String> {
        *self.replayer.lock().unwrap() = None;
        match Recorder::open(path) {
            Ok(r) => {
                *self.recorder.lock().unwrap() = Some(r);
                info!("recording keypad to {}", path);
                Ok(())
            }
            Err(e) => Err(format!("could not open {} for recording: {}", path, e)),
        }
    }

    /// Flushes and closes the active recorder, if any.
    pub fn stop_recording(&self) {
        if let Some(mut r) = self.recorder.lock().unwrap().take() {
            let _ = r.flush();
            info!("recording stopped");
        }
    }

    /// Load a recording from `path` and start feeding its events into the
    /// emulator. Stops any active recording session first.
    pub fn start_replay(&self, path: &str) -> Result<(), String> {
        *self.recorder.lock().unwrap() = None;
        match Replayer::load(path) {
            Ok(r) => {
                info!("replaying {} events from {}", r.events.len(), path);
                *self.replayer.lock().unwrap() = Some(r);
                Ok(())
            }
            Err(e) => Err(format!("could not open {} for replay: {}", path, e)),
        }
    }

    /// Stop an active replay session.
    pub fn stop_replay(&self) {
        let _ = self.replayer.lock().unwrap().take();
        info!("replay stopped");
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
