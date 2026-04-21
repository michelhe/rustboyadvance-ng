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
#[cfg_attr(test, derive(Debug))]
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

        // Tried pinning this thread to the big cluster and to the upper
        // half of cores via sched_setaffinity(). Measured worse than
        // letting Android's scheduler balance naturally: pinning to
        // just cores 6-7 (Pixel 10 Cortex-X925) dropped steady state
        // turbo from ~800 to ~560 FPS (thermal throttle concentrated
        // on two cores), and pinning to the upper half 4-7 still came
        // in lower than unpinned. The EAS scheduler makes better
        // choices than a static mask for this workload.

        let mut fps_counter = FpsCounter::default();

        // Turbo mode frame skip: in turbo, render only every Nth emulated
        // frame back to Java. The rest skip the renderFrame JNI round trip
        // entirely. The user still sees smooth (fast) motion and the emu
        // runs a lot faster because the 240 * 160 u32 IntArray copy
        // through JNI is a big chunk of per frame cost.
        const TURBO_FRAME_SKIP: u32 = 4;
        let mut turbo_frame_idx: u32 = 0;

        // Per second per phase timing buckets, gated behind debug_assertions
        // so release builds don't pay the 5 Instant::now per frame cost.
        // Release FPS logging remains, just without the phase breakdown.
        #[cfg(debug_assertions)]
        let mut t_keypad_us: u64 = 0;
        #[cfg(debug_assertions)]
        let mut t_frame_us: u64 = 0;
        #[cfg(debug_assertions)]
        let mut t_render_us: u64 = 0;
        #[cfg(debug_assertions)]
        let mut t_audio_us: u64 = 0;
        #[cfg(debug_assertions)]
        let mut t_other_us: u64 = 0;

        'running: loop {
            let emustate = *self.emustate.lock().unwrap();

            let (vsync, turbo) = match emustate {
                EmulationState::Initial => unsafe { std::hint::unreachable_unchecked() },
                EmulationState::Stopped => unsafe { std::hint::unreachable_unchecked() },
                EmulationState::Pausing => {
                    info!("emulation pause requested");
                    *self.emustate.lock().unwrap() = EmulationState::Paused;
                    continue;
                }
                EmulationState::Paused => continue,
                EmulationState::Stopping => break 'running,
                EmulationState::Running(turbo) => (!turbo, turbo),
            };

            let start_time = Instant::now();
            // check key state: live from the Java keypad unless a replay
            // session is active, in which case the replay file overrides.
            #[cfg(debug_assertions)]
            let t0 = Instant::now();
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
            #[cfg(debug_assertions)]
            let t1 = Instant::now();

            // run frame
            self.gba.frame();
            #[cfg(debug_assertions)]
            let t2 = Instant::now();

            // render video (skip N-1 of every N frames in turbo)
            let should_render = if turbo {
                turbo_frame_idx = turbo_frame_idx.wrapping_add(1);
                turbo_frame_idx.is_multiple_of(TURBO_FRAME_SKIP)
            } else {
                true
            };
            if should_render {
                self.render_video(env);
            }
            #[cfg(debug_assertions)]
            let t3 = Instant::now();

            // request audio worker to render the audio now
            audio_thread_tx
                .send(AudioThreadCommand::RenderAudio)
                .unwrap();
            #[cfg(debug_assertions)]
            let t4 = Instant::now();

            #[cfg(debug_assertions)]
            {
                t_keypad_us += (t1 - t0).as_micros() as u64;
                t_frame_us  += (t2 - t1).as_micros() as u64;
                t_render_us += (t3 - t2).as_micros() as u64;
                t_audio_us  += (t4 - t3).as_micros() as u64;
                t_other_us  += (t0 - start_time).as_micros() as u64;
            }

            if let Some(fps) = fps_counter.tick() {
                #[cfg(debug_assertions)]
                {
                    info!(
                        target: "RustdroidFps",
                        "FPS {} keypad={}us frame={}us render={}us audio={}us other={}us",
                        fps, t_keypad_us, t_frame_us, t_render_us, t_audio_us, t_other_us
                    );
                    t_keypad_us = 0;
                    t_frame_us = 0;
                    t_render_us = 0;
                    t_audio_us = 0;
                    t_other_us = 0;
                }
                #[cfg(not(debug_assertions))]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> String {
        let dir = std::env::temp_dir();
        dir.join(format!("rba-recorder-test-{}-{}.rec",
                         name, std::process::id())).to_string_lossy().into_owned()
    }

    #[test]
    fn record_then_replay_round_trips_edges() {
        let path = tmp_path("edges");
        let mut rec = Recorder::open(&path).expect("open for write");
        // Observe a sequence of states with idle duplicates; only edges
        // should land on disk.
        rec.observe(100, 0x01).unwrap();
        rec.observe(200, 0x01).unwrap(); // duplicate, skipped
        rec.observe(300, 0x03).unwrap();
        rec.observe(400, 0x00).unwrap();
        rec.observe(500, 0x00).unwrap(); // duplicate, skipped
        rec.flush().unwrap();
        drop(rec);

        let mut rp = Replayer::load(&path).expect("open for read");
        assert_eq!(rp.events.len(), 3);
        assert_eq!(rp.events[0], (100, 0x01));
        assert_eq!(rp.events[1], (300, 0x03));
        assert_eq!(rp.events[2], (400, 0x00));

        // apply_due semantics: the latest event whose cycle has passed wins.
        assert_eq!(rp.apply_due(50),  0x00); // before first event -> initial 0
        assert_eq!(rp.apply_due(150), 0x01);
        assert_eq!(rp.apply_due(250), 0x01); // no new event yet
        assert_eq!(rp.apply_due(350), 0x03); // reached second event
        assert_eq!(rp.apply_due(450), 0x00); // reached third event
        assert!(rp.exhausted());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn replay_rejects_bad_magic() {
        let path = tmp_path("badmagic");
        std::fs::write(&path, b"NOTAREC\0\0\0\0\0\0\0\0\0\0").unwrap();
        let err = Replayer::load(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn replay_handles_empty_recording() {
        let path = tmp_path("empty");
        let mut rec = Recorder::open(&path).expect("open");
        rec.flush().unwrap();
        drop(rec);

        let mut rp = Replayer::load(&path).expect("open");
        assert_eq!(rp.events.len(), 0);
        assert!(rp.exhausted());
        // apply_due on empty replayer returns initial 0.
        assert_eq!(rp.apply_due(100), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn record_out_of_order_states_preserved() {
        // Even if the emulator's get_key_state returns values that
        // "undo" themselves quickly, the recorder captures every edge
        // so replay matches execution.
        let path = tmp_path("flicker");
        let mut rec = Recorder::open(&path).unwrap();
        rec.observe(10, 0x02).unwrap();
        rec.observe(20, 0x00).unwrap();
        rec.observe(30, 0x02).unwrap();
        rec.observe(40, 0x00).unwrap();
        rec.flush().unwrap();
        drop(rec);

        let rp = Replayer::load(&path).unwrap();
        assert_eq!(rp.events.len(), 4);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn round_trip_binary_compatible_with_fps_bench_format() {
        // The on-device Recorder must write the exact same byte layout
        // the desktop SDL frontend emits and fps_bench's replay module
        // reads, so recordings are portable either direction.
        let path = tmp_path("format");
        let mut rec = Recorder::open(&path).unwrap();
        rec.observe(0x1111_2222_3333_4444, 0xBEEF).unwrap();
        rec.flush().unwrap();
        drop(rec);

        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[0..8], b"RBAREC01");
        // cycle: u64 LE, state: u16 LE
        let cycle = u64::from_le_bytes(raw[8..16].try_into().unwrap());
        let state = u16::from_le_bytes(raw[16..18].try_into().unwrap());
        assert_eq!(cycle, 0x1111_2222_3333_4444);
        assert_eq!(state, 0xBEEF);
        std::fs::remove_file(&path).ok();
    }
}
