use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use super::arm7tdmi;
use super::sysbus::SysBus;

mod parser;
use parser::{parse_expr, Expr, Value};

mod command;
use command::Command;

#[derive(Debug, PartialEq)]
pub enum DebuggerError {
    ParsingError(String),
    CpuError(arm7tdmi::CpuError),
    InvalidCommand(String),
    InvalidArgument(String),
    InvalidCommandFormat(String),
}

impl From<arm7tdmi::CpuError> for DebuggerError {
    fn from(e: arm7tdmi::CpuError) -> DebuggerError {
        DebuggerError::CpuError(e)
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug)]
pub struct Debugger {
    pub cpu: arm7tdmi::cpu::Core,
    pub sysbus: SysBus,
    running: bool,
    breakpoints: Vec<u32>,
    pub previous_command: Option<Command>,
}

impl Debugger {
    pub fn new(cpu: arm7tdmi::cpu::Core, sysbus: SysBus) -> Debugger {
        Debugger {
            cpu: cpu,
            sysbus: sysbus,
            breakpoints: Vec::new(),
            running: false,
            previous_command: None,
        }
    }

    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.cpu.get_next_pc();
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
            Value::Name(reg) => self.decode_reg(&reg),
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

    fn val_address(&self, arg: &Value) -> DebuggerResult<u32> {
        match arg {
            Value::Num(n) => Ok(*n),
            Value::Name(reg) => {
                let reg = self.decode_reg(&reg)?;
                Ok(self.cpu.get_reg(reg))
            }
            v => Err(DebuggerError::InvalidArgument(format!(
                "addr: expected a number or register, got {:?}",
                v
            ))),
        }
    }

    fn eval_command(&self, command: Value, args: Vec<Value>) -> DebuggerResult<Command> {
        let command = match command {
            Value::Name(command) => command,
            _ => {
                return Err(DebuggerError::InvalidCommand("expected a name".to_string()));
            }
        };

        match command.as_ref() {
            "i" | "info" => Ok(Command::Info),
            "s" | "step" => Ok(Command::SingleStep(false)),
            "sc" | "stepcycle" => Ok(Command::SingleStep(true)),
            "c" | "continue" => Ok(Command::Continue),
            "x" | "hexdump" => {
                let (addr, n) = match args.len() {
                    2 => {
                        let addr = self.val_address(&args[0])?;
                        let n = self.val_number(&args[1])?;

                        (addr, n as usize)
                    }
                    1 => {
                        let addr = self.val_address(&args[0])?;

                        (addr, 0x100)
                    }
                    0 => {
                        if let Some(Command::HexDump(addr, n)) = self.previous_command {
                            (addr + (4 * n as u32), 0x100)
                        } else {
                            (self.cpu.get_reg(15), 0x100)
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
            "d" | "disass" => {
                let (addr, n) = match args.len() {
                    2 => {
                        let addr = self.val_address(&args[0])?;
                        let n = self.val_number(&args[1])?;

                        (addr, n as usize)
                    }
                    1 => {
                        let addr = self.val_address(&args[0])?;

                        (addr, 10)
                    }
                    0 => {
                        if let Some(Command::Disass(addr, n)) = self.previous_command {
                            (addr + (4 * n as u32), 10)
                        } else {
                            (self.cpu.get_next_pc(), 10)
                        }
                    }
                    _ => {
                        return Err(DebuggerError::InvalidCommandFormat(
                            "disass [addr] [n]".to_string(),
                        ))
                    }
                };

                Ok(Command::Disass(addr, n))
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
            "bl" => Ok(Command::ListBreakpoints),
            "q" | "quit" => Ok(Command::Quit),
            "r" | "reset" => Ok(Command::Reset),
            _ => Err(DebuggerError::InvalidCommand(command)),
        }
    }

    fn eval_assignment(&mut self, lvalue: Value, rvalue: Value) -> DebuggerResult<()> {
        let lvalue = self.val_reg(&lvalue)?;
        let rvalue = self.val_address(&rvalue)?;

        self.cpu.set_reg(lvalue, rvalue);
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
        rl.load_history(".rustboyadvance_history");
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
