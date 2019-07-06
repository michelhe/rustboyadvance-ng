use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use super::arm7tdmi::{Addr, Bus, CpuError};
use super::GameBoyAdvance;

mod parser;
use parser::{parse_expr, DerefType, Expr, Value};

mod command;
use command::Command;

mod palette_view;

#[derive(Debug, PartialEq)]
pub enum DebuggerError {
    ParsingError(String),
    CpuError(CpuError),
    InvalidCommand(String),
    InvalidArgument(String),
    InvalidCommandFormat(String),
}

impl From<CpuError> for DebuggerError {
    fn from(e: CpuError) -> DebuggerError {
        DebuggerError::CpuError(e)
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug)]
pub struct Debugger {
    pub gba: GameBoyAdvance,
    running: bool,
    breakpoints: Vec<u32>,
    pub previous_command: Option<Command>,
}

impl Debugger {
    pub fn new(gba: GameBoyAdvance) -> Debugger {
        Debugger {
            gba: gba,
            breakpoints: Vec::new(),
            running: false,
            previous_command: None,
        }
    }

    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.gba.cpu.get_next_pc();
        for bp in &self.breakpoints {
            if *bp == next_pc {
                return Some(next_pc);
            }
        }

        None
    }

    pub fn delete_breakpoint(&mut self, addr: u32) {
        self.breakpoints.retain(|&a| a != addr);
    }

    fn decode_reg(&self, s: &str) -> DebuggerResult<usize> {
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

    fn val_address(&self, arg: &Value) -> DebuggerResult<Addr> {
        match arg {
            Value::Num(n) => Ok(*n),
            Value::Identifier(reg) => {
                let reg = self.decode_reg(&reg)?;
                Ok(self.gba.cpu.get_reg(reg))
            }
            v => Err(DebuggerError::InvalidArgument(format!(
                "addr: expected a number or register, got {:?}",
                v
            ))),
        }
    }

    fn eval_assignment(&mut self, lvalue: Value, rvalue: Value) -> DebuggerResult<()> {
        let lvalue = self.val_reg(&lvalue)?;
        let rvalue = match rvalue {
            Value::Deref(addr_value, deref_type) => {
                let addr = self.val_address(&addr_value)?;
                match deref_type {
                    DerefType::Word => self.gba.sysbus.read_32(addr),
                    DerefType::HalfWord => self.gba.sysbus.read_16(addr) as u32,
                    DerefType::Byte => self.gba.sysbus.read_8(addr) as u32,
                }
            }
            _ => self.val_address(&rvalue)?,
        };
        self.gba.cpu.set_reg(lvalue, rvalue);
        Ok(())
    }

    fn eval_expr(&mut self, expr: Expr) {
        match expr {
            Expr::Command(c, a) => match self.eval_command(c, a) {
                Ok(cmd) => {
                    self.previous_command = Some(cmd.clone());
                    cmd.run(self)
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
            Expr::Assignment(lvalue, rvalue) => match self.eval_assignment(lvalue, rvalue) {
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

    pub fn repl(&mut self) -> DebuggerResult<()> {
        println!("Welcome to rustboyadvance-NG debugger 😎!\n");
        self.running = true;
        let mut rl = Editor::<()>::new();
        rl.load_history(".rustboyadvance_history").unwrap();
        while self.running {
            let readline = rl.readline(&format!("({}) ᐅ ", "rustboyadvance-dbg".bold().cyan()));
            match readline {
                Ok(line) => {
                    if line.is_empty() {
                        self.previous_command = None;
                        continue;
                    }
                    rl.add_history_entry(line.as_str());
                    let expr = parse_expr(&line);
                    match expr {
                        Ok(expr) => self.eval_expr(expr),
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
