use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use nom;
use nom::IResult;
use nom::bytes::complete::{tag, take_till1, take_till, take_while_m_n, take_while1};
use nom::combinator::map_res;

use hexdump;

use super::arm7tdmi::cpu;
use super::sysbus::SysBus;

use crate::disass::Disassembler;

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
    InvalidCommand(String)
}

impl From<cpu::CpuError> for DebuggerError {
    fn from(e: cpu::CpuError) -> DebuggerError {
        DebuggerError::CpuError(e)
    }
}

impl From<nom::Err<(&str, nom::error::ErrorKind)>> for DebuggerError {
    fn from(e: nom::Err<(&str, nom::error::ErrorKind)>) -> DebuggerError {
        DebuggerError::ParsingError("parsing of command failed".to_string())
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug, PartialEq)]
enum DebuggerCommand {
    Info,
    SingleStep,
    Continue,
    HexDump(u32, usize),
    Disass { addr: u32, num_opcodes: usize },
    AddBreakpoint(u32),
    ListBreakpoints,
    Reset,
    Quit,
    Nop,
}
use DebuggerCommand::*;

fn from_hex(input: &str) -> Result<u32, std::num::ParseIntError> {
  u32::from_str_radix(input, 16)
}

fn from_dec(input: &str) -> Result<u32, std::num::ParseIntError> {
  u32::from_str_radix(input, 10)
}

fn whitespace(input: &str) -> IResult<&str, ()> {
    let (input, _) = take_while1(char::is_whitespace)(input)?;
    Ok((input, ()))
}

fn parse_hex_num(input: &str) -> IResult<&str, u32> {
    let (input, _) = tag("0x")(input)?;
    map_res(take_while_m_n(1, 8, |c: char| c.is_digit(16)), from_hex)(input)
}

fn parse_num(input: &str) -> IResult<&str, u32> {
    map_res(take_while1(|c: char| c.is_digit(10)), from_dec)(input)
}

fn parse_word(input: &str) -> IResult<&str, &str> {
    take_till(char::is_whitespace)(input)
}

fn parse_debugger_command(input: &str) -> DebuggerResult<DebuggerCommand> {
    // TODO this code is shit!
    let (input, command_name) = parse_word(input)?;
    match command_name {
        "i" | "info" => Ok(Info),
        "s" | "step" => Ok(SingleStep),
        "c" | "continue" => Ok(Continue),
        "xxd" => {
            let (input, _) = whitespace(input).map_err(|_| DebuggerError::ParsingError("argument missing".to_string()))?;
            let (input, addr) = parse_hex_num(input)?;
            let (input, _) = whitespace(input).map_err(|_| DebuggerError::ParsingError("argument missing".to_string()))?;
            let (_, nbytes) = parse_num(input)?;
            let nbytes = nbytes as usize;
            Ok(HexDump(addr, nbytes))
        }
        "d" | "disass" => {
            let (input, _) = whitespace(input).map_err(|_| DebuggerError::ParsingError("argument missing".to_string()))?;
            let (input, addr) = parse_hex_num(input)?;
            let (input, _) = whitespace(input).map_err(|_| DebuggerError::ParsingError("argument missing".to_string()))?;
            let (_, num_opcodes) = parse_num(input)?;
            let num_opcodes = num_opcodes as usize;
            Ok(Disass{ addr, num_opcodes })
        }
        "b" | "break" => {
            let (input, _) = whitespace(input).map_err(|_| DebuggerError::ParsingError("argument missing".to_string()))?;
            let (_, addr) = parse_hex_num(input)?;
            Ok(AddBreakpoint(addr))
        }
        "bl" => Ok(ListBreakpoints),
        "q" | "quit" => Ok(Quit),
        "r" | "reset" => Ok(Reset),
        "" => Ok(Nop),
        _ => Err(DebuggerError::InvalidCommand(command_name.to_string()))
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
                Nop => (),
                Info => {
                    println!("cpu info: {:#x?}", self.cpu)
                }
                SingleStep => {
                    ;
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
                },
                Continue => {
                    loop {
                        match self.cpu.step(&mut self.sysbus) {
                            Ok(_) => (),
                            Err(e) => {
                                println!("{}: {}", "cpu encountered an error".red(), e);
                                println!("cpu: {:#x?}", self.cpu);
                                break
                            }
                        };
                        if self.is_breakpoint_reached() {
                            println!("breakpoint 0x{:08x} reached!", self.cpu.pc)
                        }
                    }
                },
                HexDump(addr, nbytes) => {
                    let bytes = match self.sysbus.get_bytes(addr, nbytes) {
                        Some(bytes) => bytes,
                        None => {
                            println!("requested content out of bounds");
                            return
                        }
                    };
                    hexdump::hexdump(bytes);
                }
                Disass{ addr, num_opcodes } => {
                    let bytes = match self.sysbus.get_bytes(addr, 4*num_opcodes) {
                        Some(bytes) => bytes,
                        None => {
                            println!("requested content out of bounds");
                            return
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
                },
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
                },
                _ => panic!("command {:?} not implemented", cmd)
        }
    }
    pub fn repl(&mut self) -> DebuggerResult<()> {
        self.running = true;
        let mut rl = Editor::<()>::new();
        while self.running {
            let readline = rl.readline(&format!("({}) >> ", "rustboyadvance-dbg".cyan()));
            match readline {
                Ok(line) => {
                    let command = parse_debugger_command(&line);
                    match command {
                        Ok(cmd) => {
                            self.command(cmd)
                        }
                        Err(DebuggerError::InvalidCommand(command)) => {
                            println!("invalid command: {}", command)
                        }
                        Err(DebuggerError::ParsingError(msg)) => {
                            println!("Parsing error: {:?}", msg)
                        }
                        Err(e) => {
                            return Err(e);
                        }
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
