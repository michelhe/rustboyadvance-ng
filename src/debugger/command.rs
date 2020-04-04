use std::path::Path;
use std::sync::atomic::Ordering;
use std::time;

use crate::core::arm7tdmi::arm::ArmInstruction;
use crate::core::arm7tdmi::thumb::ThumbInstruction;
use crate::core::arm7tdmi::CpuState;
use crate::core::{Addr, Bus};
use crate::disass::Disassembler;
use crate::util::{read_bin_file, write_bin_file};

// use super::palette_view::create_palette_view;
// use super::tile_view::create_tile_view;
use super::{parser::Value, Debugger, DebuggerError, DebuggerResult};

use ansi_term::Colour;

use hexdump;

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
pub enum Command {
    Info,
    GpuInfo,
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
}

impl Debugger {
    pub fn run_command(&mut self, command: Command) {
        use Command::*;
        #[allow(unreachable_patterns)]
        match command {
            Info => {
                println!("{}", self.gba.cpu);
                println!("IME={}", self.gba.sysbus.io.intc.interrupt_master_enable);
                println!("IE={:#?}", self.gba.sysbus.io.intc.interrupt_enable);
                println!("IF={:#?}", self.gba.sysbus.io.intc.interrupt_flags);
            }
            GpuInfo => println!("GPU: {:#?}", self.gba.sysbus.io.gpu),
            Step(count) => {
                self.ctrlc_flag.store(true, Ordering::SeqCst);
                for _ in 0..count {
                    if !self.ctrlc_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    self.gba.cpu.step(&mut self.gba.sysbus);
                    while self.gba.cpu.last_executed.is_none() {
                        self.gba.cpu.step(&mut self.gba.sysbus);
                    }
                    if let Some(last_executed) = &self.gba.cpu.last_executed {
                        print!(
                            "{}\t{}",
                            Colour::Black
                                .bold()
                                .italic()
                                .on(Colour::White)
                                .paint(format!("Executed at @0x{:08x}:", last_executed.get_pc(),)),
                            last_executed
                        );
                        println!(
                            "{}",
                            Colour::Purple.dimmed().italic().paint(format!(
                                "\t\t/// Next instruction at @0x{:08x}",
                                self.gba.cpu.get_next_pc()
                            ))
                        );
                    }
                }
                println!("{}\n", self.gba.cpu);
            }
            Continue => {
                self.ctrlc_flag.store(true, Ordering::SeqCst);
                while self.ctrlc_flag.load(Ordering::SeqCst) {
                    self.gba.key_poll();
                    match self.gba.check_breakpoint() {
                        Some(addr) => {
                            println!("Breakpoint reached! @{:x}", addr);
                            break;
                        }
                        _ => {
                            self.gba.cpu.step(&mut self.gba.sysbus);
                        }
                    }
                }
            }
            Frame(count) => {
                let start = time::Instant::now();
                for _ in 0..count {
                    self.gba.frame();
                }
                let end = time::Instant::now();
                println!("that took {:?} seconds", end - start);
            }
            HexDump(addr, nbytes) => {
                let bytes = self.gba.sysbus.get_bytes(addr..addr + nbytes);
                hexdump::hexdump(&bytes);
            }
            MemWrite(size, addr, val) => match size {
                MemWriteCommandSize::Byte => self.gba.sysbus.write_8(addr, val as u8),
                MemWriteCommandSize::Half => self.gba.sysbus.write_16(addr, val as u16),
                MemWriteCommandSize::Word => self.gba.sysbus.write_32(addr, val as u32),
            },
            Disass(mode, addr, n) => {
                let bytes = self.gba.sysbus.get_bytes(addr..addr + n);
                match mode {
                    DisassMode::ModeArm => {
                        let disass = Disassembler::<ArmInstruction>::new(addr, &bytes);
                        for (_, line) in disass.take(n as usize) {
                            println!("{}", line)
                        }
                    }
                    DisassMode::ModeThumb => {
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
            AddBreakpoint(addr) => match self.gba.add_breakpoint(addr) {
                Some(index) => println!("Added breakpoint [{}] 0x{:08x}", index, addr),
                None => println!("Breakpint already exists."),
            },
            DelBreakpoint(addr) => self.delete_breakpoint(addr),
            ClearBreakpoints => self.gba.cpu.breakpoints.clear(),
            ListBreakpoints => {
                println!("breakpoint list:");
                for (i, b) in self.gba.cpu.breakpoints.iter().enumerate() {
                    println!("[{}] 0x{:08x}", i, b)
                }
            }
            // PaletteView => create_palette_view(&self.gba.sysbus.palette_ram.mem),
            // TileView(bg) => create_tile_view(bg, &self.gba),
            Reset => {
                println!("resetting cpu...");
                self.gba.cpu.reset(&mut self.gba.sysbus);
                println!("cpu is restarted!")
            }
            TraceToggle(flags) => {
                if flags.contains(TraceFlags::TRACE_OPCODE) {
                    self.gba.cpu.trace_opcodes = !self.gba.cpu.trace_opcodes;
                    println!(
                        "[*] opcode tracing {}",
                        if self.gba.cpu.trace_opcodes {
                            "on"
                        } else {
                            "off"
                        }
                    )
                }
                if flags.contains(TraceFlags::TRACE_EXCEPTIONS) {
                    self.gba.cpu.trace_exceptions = !self.gba.cpu.trace_exceptions;
                    println!(
                        "[*] exception tracing {}",
                        if self.gba.cpu.trace_exceptions {
                            "on"
                        } else {
                            "off"
                        }
                    )
                }
                if flags.contains(TraceFlags::TRACE_DMA) {
                    println!("[*] dma tracing not implemented");
                }
                if flags.contains(TraceFlags::TRACE_TIMERS) {
                    self.gba.sysbus.io.timers.trace = !self.gba.sysbus.io.timers.trace;
                }
            }
            SaveState(save_path) => {
                let state = self.gba.save_state().expect("failed to serialize");
                write_bin_file(&Path::new(&save_path), &state)
                    .expect("failed to save state to file");
            }
            LoadState(load_path) => {
                let save = read_bin_file(&Path::new(&load_path))
                    .expect("failed to read save state from file");
                self.gba
                    .restore_state(&save)
                    .expect("failed to deserialize");
            }
            _ => println!("Not Implemented",),
        }
    }

    fn get_disassembler_args(&self, args: Vec<Value>) -> DebuggerResult<(Addr, u32)> {
        match args.len() {
            2 => {
                let addr = self.val_address(&args[0])?;
                let n = self.val_number(&args[1])?;

                Ok((addr, n))
            }
            1 => {
                let addr = self.val_address(&args[0])?;

                Ok((addr, 10))
            }
            0 => {
                if let Some(Command::Disass(_mode, addr, n)) = &self.previous_command {
                    Ok((*addr + (4 * (*n as u32)), 10))
                } else {
                    Ok((self.gba.cpu.get_next_pc(), 10))
                }
            }
            _ => {
                return Err(DebuggerError::InvalidCommandFormat(
                    "disass [addr] [n]".to_string(),
                ))
            }
        }
    }

    pub fn eval_command(&self, command: Value, args: Vec<Value>) -> DebuggerResult<Command> {
        let command = match command {
            Value::Identifier(command) => command,
            _ => {
                return Err(DebuggerError::InvalidCommand("expected a name".to_string()));
            }
        };

        match command.as_ref() {
            "i" | "info" => Ok(Command::Info),
            "gpuinfo" => Ok(Command::GpuInfo),
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
                        let addr = self.val_address(&args[0])?;
                        let n = self.val_number(&args[1])?;

                        (addr, n)
                    }
                    1 => {
                        let addr = self.val_address(&args[0])?;

                        (addr, 0x100)
                    }
                    0 => {
                        if let Some(Command::HexDump(addr, n)) = self.previous_command {
                            (addr + (4 * n as u32), 0x100)
                        } else {
                            (self.gba.cpu.get_reg(15), 0x100)
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
                        let addr = self.val_address(&args[0])?;
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
                        let addr = self.val_address(&args[0])?;
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
                        let addr = self.val_address(&args[0])?;
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
                let (addr, n) = self.get_disassembler_args(args)?;

                let m = match self.gba.cpu.get_cpu_state() {
                    CpuState::ARM => DisassMode::ModeArm,
                    CpuState::THUMB => DisassMode::ModeThumb,
                };
                Ok(Command::Disass(m, addr, n))
            }
            "da" | "disass-arm" => {
                let (addr, n) = self.get_disassembler_args(args)?;

                Ok(Command::Disass(DisassMode::ModeArm, addr, n))
            }
            "dt" | "disass-thumb" => {
                let (addr, n) = self.get_disassembler_args(args)?;

                Ok(Command::Disass(DisassMode::ModeThumb, addr, n))
            }
            "b" | "break" => {
                if args.len() != 1 {
                    Err(DebuggerError::InvalidCommandFormat(
                        "break <addr>".to_string(),
                    ))
                } else {
                    let addr = self.val_address(&args[0])?;
                    Ok(Command::AddBreakpoint(addr))
                }
            }
            "bd" | "breakdel" => match args.len() {
                0 => Ok(Command::ClearBreakpoints),
                1 => {
                    let addr = self.val_address(&args[0])?;
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
            _ => Err(DebuggerError::InvalidCommand(command)),
        }
    }
}
