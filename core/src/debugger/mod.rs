use std::collections::HashMap;
use std::fs::File;
use std::io::{prelude::*, BufReader};

use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use super::GameBoyAdvance;
use super::{Addr, Bus};

mod parser;
use parser::{parse_expr, DerefType, Expr, Value};

mod command;
use command::Command;

mod palette_view;
mod tile_view;

#[derive(Debug)]
pub enum DebuggerError {
    ParsingError(String),
    InvalidCommand(String),
    InvalidArgument(String),
    InvalidCommandFormat(String),
    IoError(::std::io::Error),
}

impl From<::std::io::Error> for DebuggerError {
    fn from(e: ::std::io::Error) -> DebuggerError {
        DebuggerError::IoError(e)
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

pub struct Debugger {
    running: bool,
    pub previous_command: Option<Command>,
    pub symbols: HashMap<String, u32>,
}

impl Debugger {
    pub fn new() -> Debugger {
        Debugger {
            running: false,
            previous_command: None,
            symbols: HashMap::new(),
        }
    }

    pub fn check_breakpoint(&self, gba: &GameBoyAdvance) -> Option<u32> {
        let next_pc = gba.cpu.get_next_pc();
        for bp in &gba.cpu.dbg.breakpoints {
            if *bp == next_pc {
                return Some(next_pc);
            }
        }

        None
    }

    pub fn delete_breakpoint(&mut self, gba: &mut GameBoyAdvance, addr: u32) {
        gba.cpu.dbg.breakpoints.retain(|&a| a != addr);
    }

    fn decode_reg(&self, s: &str) -> DebuggerResult<usize> {
        // TODO also allow r11..r15
        let reg_names = vec![
            "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp",
            "lr", "pc",
        ];

        match reg_names.into_iter().position(|r| r == s) {
            Some(index) => Ok(index),
            None => Err(DebuggerError::InvalidArgument(format!(
                "{:?} is not a register name",
                s
            ))),
        }
    }

    fn val_reg(&self, arg: &Value) -> DebuggerResult<usize> {
        match arg {
            Value::Identifier(reg) => self.decode_reg(&reg),
            v => Err(DebuggerError::InvalidArgument(format!(
                "expected a number, got {:?}",
                v
            ))),
        }
    }

    fn val_number(&self, arg: &Value) -> DebuggerResult<u32> {
        match arg {
            Value::Num(n) => Ok(*n),
            v => Err(DebuggerError::InvalidArgument(format!(
                "expected a number, got {:?}",
                v
            ))),
        }
    }

    fn val_address(&self, gba: &GameBoyAdvance, arg: &Value) -> DebuggerResult<Addr> {
        match arg {
            Value::Num(n) => Ok(*n),
            Value::Identifier(ident) => {
                let symbol = if let Some(symbols) = gba.sysbus.cartridge.get_symbols() {
                    symbols.get(ident)
                } else {
                    None
                };

                if let Some(address) = symbol {
                    Ok(*address)
                } else {
                    // otherwise, decode as register (TODO special token to separate symbol and register)
                    let reg = self.decode_reg(&ident)?;
                    Ok(gba.cpu.get_reg(reg))
                }
            }
            v => Err(DebuggerError::InvalidArgument(format!(
                "addr: expected a number or register, got {:?}",
                v
            ))),
        }
    }

    fn eval_assignment(
        &mut self,
        gba: &mut GameBoyAdvance,
        lvalue: Value,
        rvalue: Value,
    ) -> DebuggerResult<()> {
        let lvalue = self.val_reg(&lvalue)?;
        let rvalue = match rvalue {
            Value::Deref(addr_value, deref_type) => {
                let addr = self.val_address(gba, &addr_value)?;
                match deref_type {
                    DerefType::Word => gba.sysbus.read_32(addr),
                    DerefType::HalfWord => gba.sysbus.read_16(addr) as u32,
                    DerefType::Byte => gba.sysbus.read_8(addr) as u32,
                }
            }
            _ => self.val_address(gba, &rvalue)?,
        };
        gba.cpu.set_reg(lvalue, rvalue);
        Ok(())
    }

    fn eval_expr(&mut self, gba: &mut GameBoyAdvance, expr: Expr) {
        match expr {
            Expr::Command(c, a) => match self.eval_command(gba, c, a) {
                Ok(cmd) => {
                    self.previous_command = Some(cmd.clone());
                    self.run_command(gba, cmd)
                }
                Err(DebuggerError::InvalidCommand(c)) => {
                    println!("{}: {:?}", "invalid command".red(), c)
                }
                Err(DebuggerError::InvalidArgument(m)) => {
                    println!("{}: {}", "invalid argument".red(), m)
                }
                Err(DebuggerError::InvalidCommandFormat(m)) => {
                    println!("help: {}", m.bright_yellow())
                }
                Err(e) => println!("{} {:?}", "failed to build command".red(), e),
            },
            Expr::Assignment(lvalue, rvalue) => match self.eval_assignment(gba, lvalue, rvalue) {
                Err(DebuggerError::InvalidArgument(m)) => {
                    println!("{}: {}", "assignment error".red(), m)
                }
                _ => (),
            },
            Expr::Empty => println!("Got empty expr"),
        }
    }

    fn stop(&mut self) {
        self.running = false;
    }

    pub fn repl(
        &mut self,
        gba: &mut GameBoyAdvance,
        script_file: Option<&str>,
    ) -> DebuggerResult<()> {
        println!("Welcome to rustboyadvance-NG debugger 😎!\n");
        self.running = true;
        let mut rl = Editor::<()>::new();
        let _ = rl.load_history(".rustboyadvance_history");
        if let Some(path) = script_file {
            println!("Executing script file");
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let expr = parse_expr(&line.unwrap())?;
                self.eval_expr(gba, expr);
            }
        }
        while self.running {
            let readline = rl.readline(&format!("({}) ᐅ ", "rustboyadvance-dbg".bold().cyan()));
            match readline {
                Ok(line) => {
                    if line.is_empty() {
                        if let Some(Command::Step(1)) = self.previous_command {
                            self.run_command(gba, Command::Step(1));
                        } else {
                            self.previous_command = None;
                            continue;
                        }
                    }
                    rl.add_history_entry(line.as_str());
                    let expr = parse_expr(&line);
                    match expr {
                        Ok(expr) => self.eval_expr(gba, expr),
                        Err(DebuggerError::ParsingError(msg)) => println!("Parsing error: {}", msg),
                        _ => (),
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
        rl.save_history(".rustboyadvance_history").unwrap();
        Ok(())
    }
}
