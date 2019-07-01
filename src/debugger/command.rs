use crate::arm7tdmi::bus::Bus;
use crate::arm7tdmi::{reg_string, REG_PC};
use crate::debugger::Debugger;
use crate::disass::Disassembler;

use ansi_term::Colour;

use colored::*;
use hexdump;

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Info,
    SingleStep(bool),
    Continue,
    HexDump(u32, usize),
    Disass(u32, usize),
    AddBreakpoint(u32),
    DelBreakpoint(u32),
    ClearBreakpoints,
    ListBreakpoints,
    Reset,
    Quit,
}

impl Command {
    pub fn run(&self, debugger: &mut Debugger) {
        use Command::*;
        match *self {
            Info => println!("{}", debugger.cpu),
            SingleStep(_cycle) => {
                if let Some(bp) = debugger.check_breakpoint() {
                    println!("hit breakpoint #0x{:08x}!", bp);
                    debugger.delete_breakpoint(bp);
                } else {
                    match debugger.cpu.step_debugger(&mut debugger.sysbus) {
                        Ok(insn) => {
                            println!("{}\n", debugger.cpu);
                            println!(
                                "Executed at @0x{:08x}:\n\t{}",
                                insn.pc,
                                Colour::Yellow.italic().paint(format!("{} ", insn))
                            );
                            println!("Next instruction at @0x{:08x}", debugger.cpu.get_next_pc())
                        }
                        Err(e) => {
                            println!("{}: {}", "cpu encountered an error".red(), e);
                            println!("cpu: {:x?}", debugger.cpu)
                        }
                    }
                }
            }
            Continue => loop {
                if let Some(bp) = debugger.check_breakpoint() {
                    println!("hit breakpoint #0x{:08x}!", bp);
                    debugger.delete_breakpoint(bp);
                    break;
                }
                match debugger.cpu.step_debugger(&mut debugger.sysbus) {
                    Ok(insn) => {
                        println!(
                            "@0x{:08x}:\n\t{}",
                            insn.pc,
                            Colour::Yellow.italic().paint(format!("{} ", insn))
                        );
                    }
                    Err(e) => {
                        println!("{}: {}", "cpu encountered an error".red(), e);
                        println!("cpu: {:x?}", debugger.cpu);
                        break;
                    }
                };
            },
            HexDump(addr, nbytes) => {
                let bytes = debugger.sysbus.get_bytes(addr, nbytes);
                hexdump::hexdump(bytes);
            }
            Disass(addr, n) => {
                let bytes = debugger.sysbus.get_bytes(addr, 4 * n);
                let disass = Disassembler::new(addr, bytes);
                for (_, line) in disass {
                    println!("{}", line)
                }
            }
            Quit => {
                print!("Quitting!");
                debugger.stop();
            }
            AddBreakpoint(addr) => {
                if !debugger.breakpoints.contains(&addr) {
                    let new_index = debugger.breakpoints.len();
                    debugger.breakpoints.push(addr);
                    println!("added breakpoint [{}] 0x{:08x}", new_index, addr);
                } else {
                    println!("breakpoint already exists!")
                }
            }
            DelBreakpoint(addr) => debugger.delete_breakpoint(addr),
            ClearBreakpoints => debugger.breakpoints.clear(),
            ListBreakpoints => {
                println!("breakpoint list:");
                for (i, b) in debugger.breakpoints.iter().enumerate() {
                    println!("[{}] 0x{:08x}", i, b)
                }
            }
            Reset => {
                println!("resetting cpu...");
                debugger.cpu.reset();
                println!("cpu is restarted!")
            }
        }
    }
}
