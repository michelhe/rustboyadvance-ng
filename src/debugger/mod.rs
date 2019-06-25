use std::convert::TryFrom;

use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use hexdump;

use super::arm7tdmi::cpu;
use super::sysbus::SysBus;

use crate::disass::Disassembler;

mod parser;
use parser::{parse_command_line, ParsedLine, Argument};

#[derive(Debug)]
pub struct Debugger {
    cpu: cpu::Core,
    sysbus: SysBus,
    running: bool,
    breakpoints: Vec<u32>,
}

#[derive(Debug, PartialEq)]
pub enum DebuggerError {
    ParsingError(String),
    CpuError(cpu::CpuError),
    InvalidCommand(String),
    InvalidArgument(String)
}

impl From<cpu::CpuError> for DebuggerError {
    fn from(e: cpu::CpuError) -> DebuggerError {
        DebuggerError::CpuError(e)
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug, PartialEq)]
pub enum DebuggerCommand {
    Info,
    SingleStep,
    Continue,
    HexDump(u32, usize),
    Disass { addr: u32, num_opcodes: usize },
    AddBreakpoint(u32),
    ListBreakpoints,
    Reset,
    Quit,
}
use DebuggerCommand::*;

impl TryFrom<ParsedLine> for DebuggerCommand {
    type Error = DebuggerError;

    fn try_from(value: ParsedLine) -> Result<Self, Self::Error> {
        match value.command.as_ref() {
            "i" | "info" => {
                Ok(Info)
                },
            "s" | "step" => Ok(SingleStep),
            "c" | "continue" => Ok(Continue),
            "xxd" => {
                let mut args = value.args.into_iter();
                let addr = match args.next() {
                    Some(Argument::Num(n)) => n,
                    _ => { 
                        return Err(DebuggerError::InvalidArgument(format!("expected a number")));
                    }
                };
                let nbytes = match args.next() {
                    Some(Argument::Num(n)) => n,
                    _ => { 
                        return Err(DebuggerError::InvalidArgument(format!("expected a number")));
                    }
                };
                let nbytes = nbytes as usize;
                Ok(HexDump(addr, nbytes))
            }
            "d" | "disass" => {
                let mut args = value.args.into_iter();
                let addr = match args.next() {
                    Some(Argument::Num(n)) => n,
                    _ => { 
                        return Err(DebuggerError::InvalidArgument(format!("expected a number")));
                    }
                };
                let num_opcodes = match args.next() {
                    Some(Argument::Num(n)) => n,
                    _ => { 
                        return Err(DebuggerError::InvalidArgument(format!("expected a number")));
                    }
                };
                let num_opcodes = num_opcodes as usize;
                Ok(Disass { addr, num_opcodes })
            }
            "b" | "break" => {
                let mut args = value.args.into_iter();
                let addr = match args.next() {
                    Some(Argument::Num(n)) => n,
                    _ => { 
                        return Err(DebuggerError::InvalidArgument(format!("expected a number")));
                    }
                };
                Ok(AddBreakpoint(addr))
            }
            "bl" => Ok(ListBreakpoints),
            "q" | "quit" => Ok(Quit),
            "r" | "reset" => Ok(Reset),
            _ => Err(DebuggerError::InvalidCommand(value.command)),
        }
    }
}

impl Debugger {
    pub fn new(cpu: cpu::Core, sysbus: SysBus) -> Debugger {
        Debugger {
            cpu: cpu,
            sysbus: sysbus,
            breakpoints: Vec::new(),
            running: false,
        }
    }

    fn is_breakpoint_reached(&self) -> bool {
        let pc = self.cpu.pc;
        for b in &self.breakpoints {
            if *b == pc {
                return true;
            }
        }

        false
    }

    fn command(&mut self, cmd: DebuggerCommand) {
        match cmd {
            Info => println!("cpu info: {:#x?}", self.cpu),
            SingleStep => {
                match self.cpu.step(&mut self.sysbus) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("{}: {}", "cpu encountered an error".red(), e);
                        println!("cpu: {:#x?}", self.cpu);
                    }
                };
                if self.is_breakpoint_reached() {
                    println!("breakpoint 0x{:08x} reached!", self.cpu.pc)
                }
            }
            Continue => loop {
                match self.cpu.step(&mut self.sysbus) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("{}: {}", "cpu encountered an error".red(), e);
                        println!("cpu: {:#x?}", self.cpu);
                        break;
                    }
                };
                if self.is_breakpoint_reached() {
                    println!("breakpoint 0x{:08x} reached!", self.cpu.pc)
                }
            },
            HexDump(addr, nbytes) => {
                let bytes = match self.sysbus.get_bytes(addr, nbytes) {
                    Some(bytes) => bytes,
                    None => {
                        println!("requested content out of bounds");
                        return;
                    }
                };
                hexdump::hexdump(bytes);
            }
            Disass { addr, num_opcodes } => {
                let bytes = match self.sysbus.get_bytes(addr, 4 * num_opcodes) {
                    Some(bytes) => bytes,
                    None => {
                        println!("requested content out of bounds");
                        return;
                    }
                };
                let disass = Disassembler::new(addr, bytes);

                for line in disass {
                    println!("{}", line)
                }
            }
            Quit => {
                print!("Quitting!");
                self.running = false;
            }
            AddBreakpoint(addr) => {
                if !self.breakpoints.contains(&addr) {
                    let new_index = self.breakpoints.len();
                    self.breakpoints.push(addr);
                    println!("added breakpoint [{}] 0x{:08x}", new_index, addr);
                } else {
                    println!("breakpoint already exists!")
                }
            }
            ListBreakpoints => {
                println!("breakpoint list:");
                for (i, b) in self.breakpoints.iter().enumerate() {
                    println!("[{}] 0x{:08x}", i, b)
                }
            }
            Reset => {
                println!("resetting cpu...");
                self.cpu.reset();
                println!("cpu is restarted!")
            }
            _ => panic!("command {:?} not implemented", cmd),
        }
    }
    pub fn repl(&mut self) -> DebuggerResult<()> {
        self.running = true;
        let mut rl = Editor::<()>::new();
        while self.running {
            let readline = rl.readline(&format!("({}) >> ", "rustboyadvance-dbg".cyan()));
            match readline {
                Ok(line) => {
                    if line.is_empty() {
                        continue
                    }
                    let line = parse_command_line(&line);
                    match line {
                        Ok(line) => {
                            match DebuggerCommand::try_from(line) {
                                Ok(cmd) => self.command(cmd),
                                Err(DebuggerError::InvalidCommand(c)) => println!("{}: {:?}", "invalid command".red(), c),
                                Err(DebuggerError::InvalidArgument(m)) => println!("{}: {}", "invalid argument".red(), m),
                                _ => ()
                            }
                        }
                        Err(DebuggerError::ParsingError(msg)) => {
                            println!("Parsing error: {}", msg)
                        }
                        _ => ()
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
        Ok(())
    }
}
