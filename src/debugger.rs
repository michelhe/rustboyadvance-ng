use std::str::FromStr;

use rustyline::error::ReadlineError;
use rustyline::Editor;

use nom;
use nom::bytes;
use nom::IResult;

use super::arm7tdmi::arm;
use super::arm7tdmi::cpu;
use super::sysbus::SysBus;

#[derive(Debug)]
pub struct Debugger {
    cpu: cpu::Core,
    sysbus: SysBus,
}

#[derive(Debug, PartialEq)]
pub enum DebuggerError {
    ParsingError,
}

impl From<nom::Err<(&str, nom::error::ErrorKind)>> for DebuggerError {
    fn from(e: nom::Err<(&str, nom::error::ErrorKind)>) -> DebuggerError {
        DebuggerError::ParsingError
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug, PartialEq)]
enum DebuggerCommand {
    SingleStep,
    Continue,
    Disass { addr: u32, num_opcodes: u32 },
    Stop,
    Nop,
}

fn parse_debugger_command(input: &str) -> IResult<&str, DebuggerCommand> {
    let (input, command_name) = bytes::complete::take_while1(|c: char| c.is_alphanumeric())(input)?;
    println!("parsed command: {}", command_name);

    unimplemented!()
}

impl FromStr for DebuggerCommand {
    type Err = DebuggerError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        
    }
}

impl Debugger {
    pub fn new(cpu: cpu::Core, sysbus: SysBus) -> Debugger {
        Debugger {
            cpu: cpu,
            sysbus: sysbus,
        }
    }

    pub fn repl(&self) -> DebuggerResult<()> {
        let mut rl = Editor::<()>::new();
        loop {
            let readline = rl.readline("(rustboyadvance-dbg) >> ");
            match readline {
                Ok(line) => {
                    let command = parse_debugger_command(&line)?;
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
