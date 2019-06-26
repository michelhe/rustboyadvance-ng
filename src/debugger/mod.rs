use rustyline::error::ReadlineError;
use rustyline::Editor;

use colored::*;

use hexdump;

use super::arm7tdmi::cpu;
use super::sysbus::SysBus;

use crate::disass::Disassembler;

mod parser;
use parser::{parse_expr, Expr, Value};

#[derive(Debug, PartialEq)]
pub enum DebuggerError {
    ParsingError(String),
    CpuError(cpu::CpuError),
    InvalidCommand(String),
    InvalidArgument(String),
    InvalidCommandFormat(String),
}

impl From<cpu::CpuError> for DebuggerError {
    fn from(e: cpu::CpuError) -> DebuggerError {
        DebuggerError::CpuError(e)
    }
}

type DebuggerResult<T> = Result<T, DebuggerError>;

#[derive(Debug, PartialEq)]
pub enum Command {
    Info,
    SingleStep,
    Continue,
    HexDump(u32, usize),
    Disass(u32, usize),
    AddBreakpoint(u32),
    ListBreakpoints,
    Reset,
    Quit,
}

impl Command {
    fn run(&self, debugger: &mut Debugger) {
        use Command::*;
        match *self {
            Info => println!("{}", debugger.cpu),
            SingleStep => {
                match debugger.cpu.step(&mut debugger.sysbus) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("{}: {}", "cpu encountered an error".red(), e);
                        println!("cpu: {:#x?}", debugger.cpu);
                    }
                };
                if debugger.is_breakpoint_reached() {
                    println!("breakpoint 0x{:08x} reached!", debugger.cpu.pc)
                }
            }
            Continue => loop {
                match debugger.cpu.step(&mut debugger.sysbus) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("{}: {}", "cpu encountered an error".red(), e);
                        println!("cpu: {:#x?}", debugger.cpu);
                        break;
                    }
                };
                if debugger.is_breakpoint_reached() {
                    println!("breakpoint 0x{:08x} reached!", debugger.cpu.pc)
                }
            },
            HexDump(addr, nbytes) => {
                let bytes = match debugger.sysbus.get_bytes(addr, nbytes) {
                    Some(bytes) => bytes,
                    None => {
                        println!("requested content out of bounds");
                        return;
                    }
                };
                hexdump::hexdump(bytes);
            }
            Disass(addr, n) => {
                let bytes = match debugger.sysbus.get_bytes(addr, 4 * n) {
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

                debugger.next_disass_addr = Some(addr + (4 * n as u32));
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

#[derive(Debug)]
pub struct Debugger {
    cpu: cpu::Core,
    sysbus: SysBus,
    running: bool,
    breakpoints: Vec<u32>,
    next_disass_addr: Option<u32>,
}

impl Debugger {
    pub fn new(cpu: cpu::Core, sysbus: SysBus) -> Debugger {
        Debugger {
            cpu: cpu,
            sysbus: sysbus,
            breakpoints: Vec::new(),
            running: false,
            next_disass_addr: None,
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


    fn decode_reg(&self, s: &str) -> DebuggerResult<usize> {
        let reg_names = vec![
            "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp", "lr",
            "pc"];

        match reg_names.into_iter().position(|r| r == s) {
            Some(index) => Ok(index),
            None => Err(DebuggerError::InvalidArgument(format!("{:?} is not a register name", s)))
        }
    }

    fn val_reg(&self, arg: &Value) -> DebuggerResult<usize> {
        match arg {
            Value::Name(reg) => {
                self.decode_reg(&reg)
            },
            v => Err(DebuggerError::InvalidArgument(format!("expected a number, got {:?}", v)))
        }
    }

    fn val_number(&self, arg: &Value) -> DebuggerResult<u32> {
        match arg {
            Value::Num(n) => Ok(*n),
            v => Err(DebuggerError::InvalidArgument(format!("expected a number, got {:?}", v)))
        }
    }

    fn val_address(&self, arg: &Value) -> DebuggerResult<u32> {
        match arg {
            Value::Num(n) => Ok(*n),
            Value::Name(reg) => {
                let reg = self.decode_reg(&reg)?;
                Ok(self.cpu.get_reg(reg))
            }
            v => Err(DebuggerError::InvalidArgument(format!("addr: expected a number or register, got {:?}", v)))
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
            "s" | "step" => Ok(Command::SingleStep),
            "c" | "continue" => Ok(Command::Continue),
            "xxd" => {
                let addr = self.val_address(&args[0])?;
                let nbytes = self.val_number(&args[1])?;
                Ok(Command::HexDump(addr, nbytes as usize))
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
                    0 => (self.next_disass_addr.unwrap_or(self.cpu.get_reg(15)), 10),
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
                        "disass <addr>".to_string(),
                    ))
                } else {
                    let addr =  self.val_address(&args[0])?;
                    Ok(Command::AddBreakpoint(addr))
                }
            }
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
                Ok(cmd) => cmd.run(self),
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
                Err(DebuggerError::InvalidArgument(m)) => println!("{}: {}", "assignment error".red(), m),
                _ => ()
            }
            Expr::Empty => println!("Got empty expr"),
        }
    }

    fn stop(&mut self) {
        self.running = false;
    }

    pub fn repl(&mut self) -> DebuggerResult<()> {
        self.running = true;
        let mut rl = Editor::<()>::new();
        rl.load_history(".rustboyadvance_history");
        while self.running {
            let readline = rl.readline(&format!("({}) >> ", "rustboyadvance-dbg".cyan()));
            match readline {
                Ok(line) => {
                    if line.is_empty() {
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
