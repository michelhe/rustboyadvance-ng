use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time;

use crate::arm7tdmi::arm::ArmInstruction;
use crate::arm7tdmi::thumb::ThumbInstruction;
use crate::arm7tdmi::CpuState;
use crate::bus::{Addr, Bus, DebugRead};
use crate::disass::Disassembler;
use crate::util::{read_bin_file, write_bin_file};

// use super::palette_view::create_palette_view;
// use super::tile_view::create_tile_view;
use super::GameBoyAdvance;
use super::{parser::Value, Debugger, DebuggerError, DebuggerResult};

use ansi_term::Colour;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use hexdump;

use goblin;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DisassMode {
    ModeArm,
    ModeThumb,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MemWriteCommandSize {
    Byte,
    Half,
    Word,
}

bitflags! {
    pub struct TraceFlags: u32 {
        const TRACE_SYSBUS = 0b00000001;
        const TRACE_OPCODE = 0b00000010;
        const TRACE_DMA = 0b00000100;
        const TRACE_TIMERS = 0b000001000;
        const TRACE_EXCEPTIONS = 0b000001000;
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum InfoCommand {
    Cpu,
    Gpu,
    Gpio,
    Interrupt,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Info(InfoCommand),
    Step(usize),
    Continue,
    Frame(usize),
    HexDump(Addr, u32),
    MemWrite(MemWriteCommandSize, Addr, u32),
    Disass(DisassMode, Addr, u32),
    AddBreakpoint(Addr),
    DelBreakpoint(Addr),
    // PaletteView,
    // TileView(u32),
    ClearBreakpoints,
    ListBreakpoints,
    Reset,
    Quit,
    TraceToggle(TraceFlags),
    SaveState(String),
    LoadState(String),
    AddSymbolsFile(PathBuf, Option<u32>),
    ListSymbols(Option<String>),
}

fn find_nearest_symbol(addr: u32, symbols: &HashMap<String, u32>) -> Option<(String, u32)> {
    let mut smallest_distance = u32::MAX;
    let mut symbol = String::new();

    for (k, v) in symbols.iter().filter(|(_k, v)| **v > addr) {
        let distance = v - addr;
        if distance < smallest_distance {
            smallest_distance = distance;
            symbol = k.to_string();
        }
    }

    if smallest_distance < 0x1000 {
        let symaddr = *symbols.get(&symbol).unwrap();
        Some((symbol, symaddr))
    } else {
        None
    }
}

impl Debugger {
    pub fn run_command(&mut self, gba: &mut GameBoyAdvance, command: Command) {
        use Command::*;
        #[allow(unreachable_patterns)]
        match command {
            Info(InfoCommand::Cpu) => {
                let pc = gba.cpu.pc;
                if let Some((sym, addr)) = find_nearest_symbol(pc, &self.symbols) {
                    println!("PC at {}+{:#x} ({:08x})", sym, addr - pc, pc);
                } else {
                    println!("PC at {:08x}", pc);
                }

                println!("{}", gba.cpu);
                // println!("IME={}", gba.io_devs.intc.interrupt_master_enable);
                // println!("IE={:#?}", gba.io_devs.intc.interrupt_enable);
                // println!("IF={:#?}", gba.io_devs.intc.interrupt_flags);
            }
            Info(InfoCommand::Gpu) => println!("{}", gba.io_devs.gpu),
            Info(InfoCommand::Interrupt) => {
                println!("IME: {:?}", gba.io_devs.intc.interrupt_master_enable);
                println!("IE: {:#?}", gba.io_devs.intc.interrupt_enable);
                println!("IF: {:#?}", gba.io_devs.intc.interrupt_flags.get());
            }
            Info(InfoCommand::Gpio) => println!("GPIO: {:#?}", gba.sysbus.cartridge.get_gpio()),
            Step(count) => {
                for _ in 0..count {
                    gba.step_debugger();
                    while gba.cpu.dbg.last_executed.is_none() {
                        gba.step_debugger();
                    }
                    if let Some(last_executed) = &gba.cpu.dbg.last_executed {
                        let pc = last_executed.get_pc();
                        let symbol =
                            self.symbols
                                .iter()
                                .find_map(|(key, &val)| if val == pc { Some(key) } else { None });

                        let text = if let Some(symbol) = symbol {
                            format!("Executed at {} @0x{:08x}:", symbol, pc)
                        } else {
                            format!("Executed at @0x{:08x}:", pc)
                        };
                        print!(
                            "{}\t{}",
                            Colour::Black.bold().italic().on(Colour::White).paint(text),
                            last_executed
                        );
                        println!(
                            "{}",
                            Colour::Purple.dimmed().italic().paint(format!(
                                "\t\t/// Next instruction at @0x{:08x}",
                                gba.cpu.get_next_pc()
                            ))
                        );
                    }
                }
                println!("cycles: {}", gba.scheduler.timestamp());
                println!("{}\n", gba.cpu);
            }
            Continue => 'running: loop {
                gba.key_poll();
                if let Some(breakpoint) = gba.step_debugger() {
                    let mut bp_sym = None;
                    if let Some(symbols) = gba.sysbus.cartridge.get_symbols() {
                        for s in symbols.keys() {
                            if symbols.get(s).unwrap() == &breakpoint {
                                bp_sym = Some(s.clone());
                            }
                        }
                    }
                    if let Some(sym) = bp_sym {
                        println!("Breakpoint reached! @{}", sym);
                    } else {
                        println!("Breakpoint reached! @{:x}", breakpoint);
                    }
                    break 'running;
                }
            },
            Frame(count) => {
                let start = time::Instant::now();
                for _ in 0..count {
                    gba.frame();
                }
                let end = time::Instant::now();
                println!("that took {:?} seconds", end - start);
            }
            HexDump(addr, nbytes) => {
                let bytes = gba.sysbus.debug_get_bytes(addr..addr + nbytes);
                hexdump::hexdump(&bytes);
            }
            MemWrite(size, addr, val) => match size {
                MemWriteCommandSize::Byte => gba.sysbus.write_8(addr, val as u8),
                MemWriteCommandSize::Half => gba.sysbus.write_16(addr, val as u16),
                MemWriteCommandSize::Word => gba.sysbus.write_32(addr, val as u32),
            },
            Disass(mode, addr, n) => {
                match mode {
                    DisassMode::ModeArm => {
                        let bytes = gba.sysbus.debug_get_bytes(addr..addr + 4 * n);
                        let disass = Disassembler::<ArmInstruction>::new(addr, &bytes);
                        for (_, line) in disass.take(n as usize) {
                            println!("{}", line)
                        }
                    }
                    DisassMode::ModeThumb => {
                        let bytes = gba.sysbus.debug_get_bytes(addr..addr + 2 * n);
                        let disass = Disassembler::<ThumbInstruction>::new(addr, &bytes);
                        for (_, line) in disass.take(n as usize) {
                            println!("{}", line)
                        }
                    }
                };
            }
            Quit => {
                print!("Quitting!");
                self.stop();
            }
            AddBreakpoint(addr) => match gba.add_breakpoint(addr) {
                Some(index) => println!("Added breakpoint [{}] 0x{:08x}", index, addr),
                None => println!("Breakpint already exists."),
            },
            DelBreakpoint(addr) => self.delete_breakpoint(gba, addr),
            ClearBreakpoints => gba.cpu.dbg.breakpoints.clear(),
            ListBreakpoints => {
                println!("breakpoint list:");
                for (i, b) in gba.cpu.dbg.breakpoints.iter().enumerate() {
                    println!("[{}] 0x{:08x}", i, b)
                }
            }
            // PaletteView => create_palette_view(&gba.sysbus.palette_ram.mem),
            // TileView(bg) => create_tile_view(bg, &gba),
            Reset => {
                println!("resetting cpu...");
                gba.cpu.reset();
                println!("cpu is restarted!")
            }
            TraceToggle(flags) => {
                if flags.contains(TraceFlags::TRACE_OPCODE) {
                    println!("[*] opcode tracing not implemented")
                }
                if flags.contains(TraceFlags::TRACE_EXCEPTIONS) {
                    gba.cpu.dbg.trace_exceptions = !gba.cpu.dbg.trace_exceptions;
                    println!(
                        "[*] exception tracing {}",
                        if gba.cpu.dbg.trace_exceptions {
                            "on"
                        } else {
                            "off"
                        }
                    )
                }
                if flags.contains(TraceFlags::TRACE_DMA) {
                    gba.sysbus.io.dmac.trace = !gba.sysbus.io.dmac.trace;
                    println!(
                        "[*] dma tracing {}",
                        if gba.sysbus.io.dmac.trace {
                            "on"
                        } else {
                            "off"
                        }
                    )
                }
                if flags.contains(TraceFlags::TRACE_TIMERS) {
                    gba.sysbus.io.timers.trace = !gba.sysbus.io.timers.trace;
                    println!(
                        "[*] timer tracing {}",
                        if gba.sysbus.io.timers.trace {
                            "on"
                        } else {
                            "off"
                        }
                    )
                }
            }
            SaveState(save_path) => {
                let state = gba.save_state().expect("failed to serialize");
                write_bin_file(&Path::new(&save_path), &state)
                    .expect("failed to save state to file");
            }
            LoadState(load_path) => {
                let save = read_bin_file(&Path::new(&load_path))
                    .expect("failed to read save state from file");
                gba.restore_state(&save).expect("failed to deserialize");
            }
            ListSymbols(Some(pattern)) => {
                let matcher = SkimMatcherV2::default();
                for (k, v) in self
                    .symbols
                    .iter()
                    .filter(|(k, _v)| matcher.fuzzy_match(k, &pattern).is_some())
                {
                    println!("{}=0x{:08x}", k, v);
                }
            }
            ListSymbols(None) => {
                for (k, v) in self.symbols.iter() {
                    println!("{}=0x{:08x}", k, v);
                }
            }
            AddSymbolsFile(elf_file, offset) => {
                let offset = offset.unwrap_or(0);
                if let Ok(elf_buffer) = read_bin_file(&elf_file) {
                    if let Ok(elf) = goblin::elf::Elf::parse(&elf_buffer) {
                        let strtab = elf.strtab;
                        for sym in elf.syms.iter() {
                            if let Some(Ok(name)) = strtab.get(sym.st_name) {
                                self.symbols
                                    .insert(name.to_owned(), offset + (sym.st_value as u32));
                            } else {
                                warn!("failed to parse symbol name sym {:?}", sym);
                            }
                        }
                    } else {
                        println!("[error] Failed to parse elf file!");
                        return;
                    }
                } else {
                    println!("[error] Can't read elf file!");
                    return;
                };
            }
            _ => println!("Not Implemented",),
        }
    }

    fn get_disassembler_args(
        &self,
        gba: &GameBoyAdvance,
        args: Vec<Value>,
    ) -> DebuggerResult<(Addr, u32)> {
        match args.len() {
            2 => {
                let addr = self.val_address(gba, &args[0])?;
                let n = self.val_number(&args[1])?;

                Ok((addr, n))
            }
            1 => {
                let addr = self.val_address(gba, &args[0])?;

                Ok((addr, 10))
            }
            0 => {
                if let Some(Command::Disass(_mode, addr, n)) = &self.previous_command {
                    Ok((*addr + (*n as u32), 10))
                } else {
                    Ok((gba.cpu.get_next_pc(), 10))
                }
            }
            _ => {
                return Err(DebuggerError::InvalidCommandFormat(
                    "disass [addr] [n]".to_string(),
                ))
            }
        }
    }

    pub fn eval_command(
        &self,
        gba: &GameBoyAdvance,
        command: Value,
        args: Vec<Value>,
    ) -> DebuggerResult<Command> {
        let command = match command {
            Value::Identifier(command) => command,
            _ => {
                return Err(DebuggerError::InvalidCommand("expected a name".to_string()));
            }
        };

        match command.as_ref() {
            "i" | "info" => {
                let usage_err =
                    DebuggerError::InvalidCommandFormat(String::from("info cpu|gpu|irq|gpio"));
                match args.len() {
                    1 => {
                        if let Value::Identifier(what) = &args[0] {
                            match what.as_ref() {
                                "cpu" => Ok(Command::Info(InfoCommand::Cpu)),
                                "gpu" => Ok(Command::Info(InfoCommand::Gpu)),
                                "irq" => Ok(Command::Info(InfoCommand::Interrupt)),
                                "gpio" => Ok(Command::Info(InfoCommand::Gpio)),
                                _ => Err(DebuggerError::InvalidArgument(String::from(
                                    "invalid argument",
                                ))),
                            }
                        } else {
                            Err(usage_err)
                        }
                    }
                    _ => Err(usage_err),
                }
            }
            "s" | "step" => {
                let count = match args.len() {
                    0 => 1,
                    1 => self.val_number(&args[0])?,
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "step [count]".to_string(),
                        ))
                    }
                };
                Ok(Command::Step(count as usize))
            }
            "c" | "continue" => Ok(Command::Continue),
            "f" | "frame" => {
                let count = match args.len() {
                    0 => 1,
                    1 => self.val_number(&args[0])?,
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "frame [count]".to_string(),
                        ))
                    }
                };
                Ok(Command::Frame(count as usize))
            }
            "x" | "hexdump" => {
                let (addr, n) = match args.len() {
                    2 => {
                        let addr = self.val_address(gba, &args[0])?;
                        let n = self.val_number(&args[1])?;

                        (addr, n)
                    }
                    1 => {
                        let addr = self.val_address(gba, &args[0])?;

                        (addr, 0x100)
                    }
                    0 => {
                        if let Some(Command::HexDump(addr, n)) = self.previous_command {
                            (addr + (4 * n as u32), 0x100)
                        } else {
                            (gba.cpu.get_reg(15), 0x100)
                        }
                    }
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "xxd [addr] [n]".to_string(),
                        ))
                    }
                };
                Ok(Command::HexDump(addr, n))
            }
            "mwb" => {
                let (addr, val) = match args.len() {
                    2 => {
                        let addr = self.val_address(gba, &args[0])?;
                        let val = self.val_number(&args[1])? as u8;

                        (addr, val)
                    }
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "mwb [addr] [n]".to_string(),
                        ))
                    }
                };
                Ok(Command::MemWrite(
                    MemWriteCommandSize::Byte,
                    addr,
                    val as u32,
                ))
            }
            "mwh" => {
                let (addr, val) = match args.len() {
                    2 => {
                        let addr = self.val_address(gba, &args[0])?;
                        let val = self.val_number(&args[1])? as u16;

                        (addr, val)
                    }
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "mwb [addr] [n]".to_string(),
                        ))
                    }
                };
                Ok(Command::MemWrite(
                    MemWriteCommandSize::Half,
                    addr,
                    val as u32,
                ))
            }
            "mww" => {
                let (addr, val) = match args.len() {
                    2 => {
                        let addr = self.val_address(gba, &args[0])?;
                        let val = self.val_number(&args[1])? as u32;

                        (addr, val)
                    }
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "mwb [addr] [n]".to_string(),
                        ))
                    }
                };
                Ok(Command::MemWrite(
                    MemWriteCommandSize::Half,
                    addr,
                    val as u32,
                ))
            }
            "d" | "disass" => {
                let (addr, n) = self.get_disassembler_args(gba, args)?;

                let m = match gba.cpu.get_cpu_state() {
                    CpuState::ARM => DisassMode::ModeArm,
                    CpuState::THUMB => DisassMode::ModeThumb,
                };
                Ok(Command::Disass(m, addr, n))
            }
            "da" | "disass-arm" => {
                let (addr, n) = self.get_disassembler_args(gba, args)?;

                Ok(Command::Disass(DisassMode::ModeArm, addr, n))
            }
            "dt" | "disass-thumb" => {
                let (addr, n) = self.get_disassembler_args(gba, args)?;

                Ok(Command::Disass(DisassMode::ModeThumb, addr, n))
            }
            "b" | "break" => {
                if args.len() != 1 {
                    Err(DebuggerError::InvalidCommandFormat(
                        "break <addr>".to_string(),
                    ))
                } else {
                    let addr = self.val_address(gba, &args[0])?;
                    Ok(Command::AddBreakpoint(addr))
                }
            }
            "bd" | "breakdel" => match args.len() {
                0 => Ok(Command::ClearBreakpoints),
                1 => {
                    let addr = self.val_address(gba, &args[0])?;
                    Ok(Command::DelBreakpoint(addr))
                }
                _ => Err(DebuggerError::InvalidCommandFormat(String::from(
                    "breakdel [addr]",
                ))),
            },
            // "palette-view" => Ok(Command::PaletteView),
            // "tiles" => {
            //     if args.len() != 1 {
            //         return Err(DebuggerError::InvalidCommandFormat("tile <bg>".to_string()));
            //     }
            //     let bg = self.val_number(&args[0])?;
            //     Ok(Command::TileView(bg))
            // }
            "bl" => Ok(Command::ListBreakpoints),
            "q" | "quit" => Ok(Command::Quit),
            "r" | "reset" => Ok(Command::Reset),
            "trace" => {
                let usage = DebuggerError::InvalidCommandFormat(String::from(
                    "trace [sysbus|opcode|dma|all|exceptions]",
                ));
                if args.len() != 1 {
                    Err(usage)
                } else {
                    if let Value::Identifier(flag_str) = &args[0] {
                        let flags = match flag_str.as_ref() {
                            "sysbus" => TraceFlags::TRACE_SYSBUS,
                            "opcode" => TraceFlags::TRACE_OPCODE,
                            "exceptions" => TraceFlags::TRACE_EXCEPTIONS,
                            "dma" => TraceFlags::TRACE_DMA,
                            "timers" => TraceFlags::TRACE_TIMERS,
                            "all" => TraceFlags::all(),
                            _ => return Err(usage),
                        };
                        Ok(Command::TraceToggle(flags))
                    } else {
                        Err(usage)
                    }
                }
            }
            "save" | "load" => {
                let usage = DebuggerError::InvalidCommandFormat(String::from("save/load <path>"));
                if args.len() != 1 {
                    Err(usage)
                } else {
                    if let Value::Identifier(path) = &args[0] {
                        match command.as_ref() {
                            "save" => Ok(Command::SaveState(path.to_string())),
                            "load" => Ok(Command::LoadState(path.to_string())),
                            _ => unreachable!(),
                        }
                    } else {
                        Err(usage)
                    }
                }
            }
            "add-symbols-file" | "load-symbols" | "load-syms" => match args.len() {
                1 => {
                    if let Value::Identifier(elf_file) = &args[0] {
                        Ok(Command::AddSymbolsFile(PathBuf::from(elf_file), None))
                    } else {
                        Err(DebuggerError::InvalidArgument(String::from(
                            "expected a filename",
                        )))
                    }
                }
                2 => {
                    if let Value::Identifier(elf_file) = &args[0] {
                        if let Value::Num(offset) = &args[1] {
                            Ok(Command::AddSymbolsFile(
                                PathBuf::from(elf_file),
                                Some(*offset),
                            ))
                        } else {
                            Err(DebuggerError::InvalidArgument(String::from(
                                "expected a number",
                            )))
                        }
                    } else {
                        Err(DebuggerError::InvalidArgument(String::from(
                            "expected a filename",
                        )))
                    }
                }
                _ => Err(DebuggerError::InvalidCommandFormat(format!(
                    "usage: {} path/to/elf [offset]",
                    command
                ))),
            },
            "list-symbols" | "list-syms" | "symbols" | "syms" => match args.len() {
                0 => Ok(Command::ListSymbols(None)),
                1 => {
                    if let Value::Identifier(pattern) = &args[0] {
                        Ok(Command::ListSymbols(Some(pattern.to_string())))
                    } else {
                        Err(DebuggerError::InvalidArgument(String::from(
                            "expected a pattern",
                        )))
                    }
                }
                _ => Err(DebuggerError::InvalidCommandFormat(format!(
                    "usage: {} [pattern]",
                    command
                ))),
            },
            _ => Err(DebuggerError::InvalidCommand(command)),
        }
    }
}
