use std::path::PathBuf;

use clap::Parser;

use rustboyadvance_core::prelude::*;
use rustboyadvance_utils::FpsCounter;

mod replay;

#[derive(Parser, Debug)]
#[command(name = "fps_bench")]
struct Options {
    /// BIOS file to use.
    bios: PathBuf,
    /// ROM file to run.
    rom: PathBuf,

    /// Path to a recording produced by `rustboyadvance-sdl2 --record-input`.
    /// If set, fps_bench runs exactly as long as the recording lasts while
    /// feeding the recorded keypad changes into the emulator at the cycle
    /// counts they were captured at, and reports aggregate FPS stats at end.
    ///
    /// If unset, fps_bench runs indefinitely and prints per-second FPS (the
    /// original "how fast is idle gameplay" benchmark).
    #[arg(long = "replay", value_name = "PATH")]
    replay: Option<PathBuf>,

    /// Loop the replay recording this many times inside one benchmark run.
    /// Useful when a single recording is short (~30s) — looping gives a
    /// multi-minute sample with much lower FPS-average jitter. Events are
    /// re-issued with their cycles shifted by (loop_index * recording_span).
    #[arg(long = "loops", value_name = "N", default_value_t = 1)]
    loops: u32,
}

fn main() {
    let opts = Options::parse();

    let bios = read_bin_file(&opts.bios).expect("failed to read bios file");
    let rom = read_bin_file(&opts.rom).expect("failed to read rom file");

    let gamepak = GamepakBuilder::new()
        .take_buffer(rom.into_boxed_slice())
        .with_sram()
        .without_backup_to_file()
        .build()
        .unwrap();

    let mut gba = GameBoyAdvance::new(bios.into_boxed_slice(), gamepak, NullAudio::new());
    gba.skip_bios();

    match opts.replay {
        Some(path) => run_replay(&mut gba, &path, opts.loops.max(1)),
        None => run_idle(&mut gba),
    }
}

/// Original benchmark loop: run forever, print FPS once per second.
fn run_idle(gba: &mut GameBoyAdvance) {
    let mut fps_counter = FpsCounter::default();
    loop {
        gba.frame();
        if let Some(fps) = fps_counter.tick() {
            println!("FPS: {}", fps);
        }
    }
}

/// Replay loop: drive the emulator with recorded keypad edges until the
/// recording runs out of events AND the emulator's cycle counter passes the
/// last recorded edge, then report aggregate timing.
///
/// `loops` > 1 re-feeds the same recording back-to-back: when the cursor
/// reaches the end, it rewinds and the event cycles are shifted forward by
/// the recording's cycle span so replay keeps working on the monotonic
/// emulator cycle counter. This lets a short 30s recording act as a
/// multi-minute low-jitter measurement.
fn run_replay(gba: &mut GameBoyAdvance, path: &std::path::Path, loops: u32) {
    let mut replayer = replay::Replayer::load(path).expect("failed to load recording");
    let recording_span = replayer.last_cycle();
    let total_cycles = recording_span.saturating_mul(loops as u64);
    eprintln!(
        "Replaying {} events × {} loop(s), terminating at cycle {}",
        replayer.len(),
        loops,
        total_cycles,
    );

    let mut fps_counter = FpsCounter::default();
    let wall_start = std::time::Instant::now();
    let mut frames: u64 = 0;
    let mut loop_idx: u32 = 0;
    // Offset added to each event's cycle stamp this loop iteration, so the
    // replayer's cycle comparisons keep working against the emulator's
    // monotonic counter even on the 2nd+ pass through the recording.
    let mut cycle_offset: u64 = 0;

    loop {
        replayer.apply_due(
            (gba.cycles() as u64).saturating_sub(cycle_offset),
            gba.get_key_state_mut(),
        );

        gba.frame();
        frames += 1;

        if let Some(fps) = fps_counter.tick() {
            println!("FPS: {}", fps);
        }

        if replayer.exhausted()
            && (gba.cycles() as u64).saturating_sub(cycle_offset) >= recording_span
        {
            loop_idx += 1;
            if loop_idx >= loops {
                break;
            }
            replayer.rewind();
            cycle_offset = cycle_offset.saturating_add(recording_span);
        }
    }

    let elapsed = wall_start.elapsed().as_secs_f64();
    let avg_fps = frames as f64 / elapsed;
    println!("---");
    println!(
        "replay done: {} frames in {:.2}s wall, {:.1} avg fps ({} emulated cycles, {} loop(s))",
        frames,
        elapsed,
        avg_fps,
        gba.cycles(),
        loops,
    );
}
