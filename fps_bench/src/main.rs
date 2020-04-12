use std::cell::RefCell;
use std::env;
use std::path::Path;
use std::rc::Rc;

use rustboyadvance_core::prelude::*;
use rustboyadvance_core::util::FpsCounter;

struct BenchmarkHardware {}

impl BenchmarkHardware {
    fn new() -> BenchmarkHardware {
        BenchmarkHardware {}
    }
}

impl VideoInterface for BenchmarkHardware {}
impl AudioInterface for BenchmarkHardware {}
impl InputInterface for BenchmarkHardware {}

fn main() {
    if env::args().count() < 3 {
        eprintln!("usage: {} <bios> <rom>", env::args().nth(0).unwrap());
        return;
    }

    let bios_path = env::args().nth(1).expect("missing <bios>");
    let rom_path = env::args().nth(2).expect("missing <rom>");

    let bios = read_bin_file(Path::new(&bios_path)).expect("failed to read bios file");
    let rom = read_bin_file(Path::new(&rom_path)).expect("failed to read rom file");

    let gamepak = GamepakBuilder::new()
        .take_buffer(rom.into_boxed_slice())
        .with_sram()
        .without_backup_to_file()
        .build()
        .unwrap();

    let dummy = Rc::new(RefCell::new(BenchmarkHardware::new()));

    let mut gba = GameBoyAdvance::new(
        bios.into_boxed_slice(),
        gamepak,
        dummy.clone(),
        dummy.clone(),
        dummy.clone(),
    );
    gba.skip_bios();

    let mut fps_counter = FpsCounter::default();
    loop {
        gba.frame();
        if let Some(fps) = fps_counter.tick() {
            println!("FPS: {}", fps);
        }
    }
}
