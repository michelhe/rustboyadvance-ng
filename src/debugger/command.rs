use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time;

use crate::core::arm7tdmi::arm::ArmInstruction;
use crate::core::arm7tdmi::bus::Bus;
use crate::core::arm7tdmi::thumb::ThumbInstruction;
use crate::core::arm7tdmi::{Addr, CpuState};
use crate::core::GBAError;
use crate::disass::Disassembler;

use super::palette_view::create_palette_view;
// use super::tile_view::create_tile_view;
use super::{parser::Value, Debugger, DebuggerError, DebuggerResult};

use ansi_term::Colour;

use colored::*;
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

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Info,
    DisplayInfo,
    Step(usize),
    Continue,
    Frame(usize),
    HexDump(Addr, u32),
    MemWrite(MemWriteCommandSize, Addr, u32),
    Disass(DisassMode, Addr, u32),
    AddBreakpoint(Addr),
    DelBreakpoint(Addr),
    PaletteView,
    // TileView(u32),
    ClearBreakpoints,
    ListBreakpoints,
    Reset,
    Quit,
}

impl Command {
    pub fn run(&self, debugger: &mut Debugger) {
        use Command::*;
        match *self {
            Info => println!("{}", debugger.gba.cpu),
            DisplayInfo => { /*println!("GPU: {:#?}", debugger.gba.sysbus.io.gpu)*/ }
            Step(count) => {
                debugger.ctrlc_flag.store(true, Ordering::SeqCst);
                for _ in 0..count {
                    if !debugger.ctrlc_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    debugger.gba.step_new();
                    while debugger.gba.cpu.last_executed.is_none() {
                        debugger.gba.step_new();
                    }
                    let last_executed = debugger.gba.cpu.last_executed.unwrap();
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
                            debugger.gba.cpu.get_next_pc()
                        ))
                    );
                }
                println!("{}\n", debugger.gba.cpu);
                // match debugger.gba.step() {
                //     Ok(insn) => {
                //         print!(
                //             "{}\t{}",
                //             Colour::Black
                //                 .bold()
                //                 .italic()
                //                 .on(Colour::White)
                //                 .paint(format!("Executed at @0x{:08x}:", insn.get_pc(),)),
                //             insn
                //         );
                //         println!(
                //             "{}",
                //             Colour::Purple.dimmed().italic().paint(format!(
                //                 "\t\t/// Next instruction at @0x{:08x}",
                //                 debugger.gba.cpu.get_next_pc()
                //             ))
                //         )
                //     }
                //     Err(GBAError::CpuError(e)) => {
                //         println!("{}: {}", "cpu encountered an error".red(), e);
                //         println!("cpu: {:x?}", debugger.gba.cpu)
                //     }
                //     _ => unreachable!(),
                // }
            }
            Continue => {
                let frame_time = time::Duration::new(0, 1_000_000_000u32 / 60);
                debugger.ctrlc_flag.store(true, Ordering::SeqCst);
                while debugger.ctrlc_flag.load(Ordering::SeqCst) {
                    let start_time = time::Instant::now();
                    debugger.gba.frame();

                    let time_passed = start_time.elapsed();
                    let delay = frame_time.checked_sub(time_passed);
                    match delay {
                        None => {}
                        Some(delay) => {
                            ::std::thread::sleep(delay);
                        }
                    };
                }
                // let start_cycles = debugger.gba.cpu.cycles();
                // loop {
                //     if let Some(bp) = debugger.check_breakpoint() {
                //         match debugger.gba.step() {
                //             Err(GBAError::CpuError(e)) => {
                //                 println!("{}: {}", "cpu encountered an error".red(), e);
                //                 println!("cpu: {:x?}", debugger.gba.cpu);
                //                 break;
                //             }
                //             _ => (),
                //         };
                //         let num_cycles = debugger.gba.cpu.cycles() - start_cycles;
                //         println!("hit breakpoint #0x{:08x} after {} cycles !", bp, num_cycles);
                //         break;
                //     } else {
                //         match debugger.gba.step() {
                //             Err(GBAError::CpuError(e)) => {
                //                 println!("{}: {}", "cpu encountered an error".red(), e);
                //                 println!("cpu: {:x?}", debugger.gba.cpu);
                //                 break;
                //             }
                //             _ => (),
                //         };
                //     }
                // }
            }
            Frame(count) => {
                use super::time::PreciseTime;
                let start = PreciseTime::now();
                for _ in 0..count {
                    debugger.gba.frame();
                }
                let end = PreciseTime::now();
                println!("that took {} seconds", start.to(end));
            }
            HexDump(addr, nbytes) => {
                let bytes = debugger.gba.sysbus.get_bytes(addr..addr + nbytes);
                hexdump::hexdump(&bytes);
            }
            MemWrite(size, addr, val) => match size {
                MemWriteCommandSize::Byte => debugger.gba.sysbus.write_8(addr, val as u8),
                MemWriteCommandSize::Half => debugger.gba.sysbus.write_16(addr, val as u16),
                MemWriteCommandSize::Word => debugger.gba.sysbus.write_32(addr, val as u32),
            },
            Disass(mode, addr, n) => {
                let bytes = debugger.gba.sysbus.get_bytes(addr..addr + n);
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
                debugger.stop();
            }
            AddBreakpoint(addr) => {
                if !debugger.gba.cpu.breakpoints.contains(&addr) {
                    let new_index = debugger.gba.cpu.breakpoints.len();
                    debugger.gba.cpu.breakpoints.push(addr);
                    println!("added breakpoint [{}] 0x{:08x}", new_index, addr);
                } else {
                    println!("breakpoint already exists!")
                }
            }
            DelBreakpoint(addr) => debugger.delete_breakpoint(addr),
            ClearBreakpoints => debugger.gba.cpu.breakpoints.clear(),
            ListBreakpoints => {
                println!("breakpoint list:");
                for (i, b) in debugger.gba.cpu.breakpoints.iter().enumerate() {
                    println!("[{}] 0x{:08x}", i, b)
                }
            }
            PaletteView => create_palette_view(&debugger.gba.sysbus.palette_ram.mem),
            // TileView(bg) => create_tile_view(bg, &debugger.gba),
            Reset => {
                println!("resetting cpu...");
                debugger.gba.cpu.reset(&mut debugger.gba.sysbus);
                println!("cpu is restarted!")
            }
        }
    }
}

impl Debugger {
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
            "dispinfo" => Ok(Command::DisplayInfo),
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

                let m = match self.gba.cpu.cpsr.state() {
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
            "palette-view" => Ok(Command::PaletteView),
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
            _ => Err(DebuggerError::InvalidCommand(command)),
        }
    }
}
