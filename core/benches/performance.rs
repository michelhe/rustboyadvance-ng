/// Measure first 60 frames bigmap.gba from tonc demos
///
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

use std::cell::RefCell;
use std::rc::Rc;

use rustboyadvance_core::prelude::*;

struct BenchmarkHardware {}
impl AudioInterface for BenchmarkHardware {}
impl InputInterface for BenchmarkHardware {}

fn create_gba() -> GameBoyAdvance {
    // TODO: do I really want this file in my repository ?
    let bios = include_bytes!("roms/normatt_gba_bios.bin");
    let bigmap_rom = include_bytes!("roms/bigmap.gba");

    let gpak = GamepakBuilder::new()
        .take_buffer(bigmap_rom.to_vec().into_boxed_slice())
        .with_sram()
        .without_backup_to_file()
        .build()
        .unwrap();

    let dummy = Rc::new(RefCell::new(BenchmarkHardware {}));

    let mut gba = GameBoyAdvance::new(
        bios.to_vec().into_boxed_slice(),
        gpak,
        dummy.clone(),
        dummy.clone(),
    );
    gba.skip_bios();
    // skip initialization of the ROM to get to a stabilized scene
    for _ in 0..60 {
        gba.frame();
    }
    gba
}

pub fn performance_benchmark(c: &mut Criterion) {
    c.bench_function("run_60_frames", |b| {
        b.iter_batched(
            // setup
            || create_gba(),
            // bencher
            |mut gba| {
                for _ in 0..60 {
                    black_box(gba.frame())
                }
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = performance_benchmark
}
criterion_main!(benches);
