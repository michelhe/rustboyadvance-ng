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
        Some(path) => run_replay(&mut gba, &path),
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
fn run_replay(gba: &mut GameBoyAdvance, path: &std::path::Path) {
    let mut replayer = replay::Replayer::load(path).expect("failed to load recording");
    let last_cycle = replayer.last_cycle();
    eprintln!(
        "Replaying {} events, terminating at cycle {}",
        replayer.len(),
        last_cycle
    );

    let mut fps_counter = FpsCounter::default();
    let wall_start = std::time::Instant::now();
    let mut frames: u64 = 0;

    loop {
        // Apply any keypad edges whose recorded cycle has already elapsed.
        // This runs BEFORE the frame so an edge whose cycle lands inside the
        // upcoming frame's cycle range is applied one frame late at worst —
        // the same granularity the recorder captures at (once per frame).
        replayer.apply_due(gba.cycles() as u64, gba.get_key_state_mut());

        gba.frame();
        frames += 1;

        if let Some(fps) = fps_counter.tick() {
            println!("FPS: {}", fps);
        }

        // Terminate once the recording is fully consumed and the emulator
        // has run past the trailing event's cycle. This ensures both builds
        // execute the same total amount of emulated work before reporting.
        if replayer.exhausted() && gba.cycles() as u64 >= last_cycle {
            break;
        }
    }

    let elapsed = wall_start.elapsed().as_secs_f64();
    let avg_fps = frames as f64 / elapsed;
    println!("---");
    println!(
        "replay done: {} frames in {:.2}s wall, {:.1} avg fps ({} emulated cycles)",
        frames,
        elapsed,
        avg_fps,
        gba.cycles()
    );
}
