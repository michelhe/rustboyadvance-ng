#[macro_use]
extern crate clap;

use clap::{App, ArgMatches};

extern crate rustboyadvance_ng;

use rustboyadvance_ng::arm7tdmi::Core;
use rustboyadvance_ng::cartridge::Cartridge;
use rustboyadvance_ng::debugger::Debugger;
use rustboyadvance_ng::util::read_bin_file;
use rustboyadvance_ng::{GBAResult, GameBoyAdvance};

fn run_debug(matches: &ArgMatches) -> GBAResult<()> {
    let bios_bin = read_bin_file(matches.value_of("bios").unwrap_or_default())?;
    let rom_bin = read_bin_file(matches.value_of("game_rom").unwrap())?;

    let gamepak = Cartridge::new(rom_bin);
    println!("loaded rom: {:#?}", gamepak.header);

    let mut core = Core::new();
    core.reset();
    core.set_verbose(true);

    let gba = GameBoyAdvance::new(core, bios_bin, gamepak);

    let mut debugger = Debugger::new(gba);

    println!("starting debugger...");
    debugger.repl()?;
    println!("ending debugger...");

    Ok(())
}

fn main() {
    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    let result = match matches.subcommand() {
        ("debug", Some(m)) => run_debug(m),
        _ => Ok(()),
    };

    if let Err(err) = result {
        println!("Got an error: {:?}", err);
    }
}
