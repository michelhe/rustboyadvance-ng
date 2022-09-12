use log::info;
use simple_logger::SimpleLogger;

use gdbstub::common::Signal;
use gdbstub::conn::{Connection, ConnectionExt};
use gdbstub::stub::{run_blocking, GdbStub, SingleThreadStopReason};
use gdbstub::target::Target;

use arm7tdmi::gdb::wait_for_connection;
use arm7tdmi::{Arm7tdmiCore, SimpleMemory};

use rustboyadvance_utils::Shared;

struct SimpleEmulator {
    cpu: Arm7tdmiCore<SimpleMemory>,
}

impl SimpleEmulator {
    fn new(program: &[u8]) -> SimpleEmulator {
        let mut memory = SimpleMemory::new(0x4000);
        memory.load_program(program);

        let bus = Shared::new(memory);
        let mut cpu = Arm7tdmiCore::new(bus);
        cpu.reset();

        SimpleEmulator { cpu }
    }
}

impl run_blocking::BlockingEventLoop for SimpleEmulator {
    type Target = Arm7tdmiCore<SimpleMemory>;
    type Connection = Box<dyn ConnectionExt<Error = std::io::Error>>;
    type StopReason = SingleThreadStopReason<u32>;

    fn wait_for_stop_reason(
        target: &mut Self::Target,
        conn: &mut Self::Connection,
    ) -> Result<
        run_blocking::Event<SingleThreadStopReason<u32>>,
        run_blocking::WaitForStopReasonError<
            <Self::Target as Target>::Error,
            <Self::Connection as Connection>::Error,
        >,
    > {
        let mut steps = 0;
        loop {
            if conn.peek().map(|b| b.is_some()).unwrap_or(true) {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                return Ok(run_blocking::Event::IncomingData(byte));
            } else {
                target.step();
                if target.check_breakpoint().is_some() {
                    return Ok(run_blocking::Event::TargetStopped(
                        SingleThreadStopReason::SwBreak(()),
                    ));
                }

                steps += 1;
                if steps % 1024 == 0 {
                    return Ok(run_blocking::Event::TargetStopped(
                        SingleThreadStopReason::SwBreak(()),
                    ));
                }
            }
        }
    }

    fn on_interrupt(
        _target: &mut Arm7tdmiCore<SimpleMemory>,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <Arm7tdmiCore<SimpleMemory> as Target>::Error>
    {
        // Because this emulator runs as part of the GDB stub loop, there isn't any
        // special action that needs to be taken to interrupt the underlying target. It
        // is implicitly paused whenever the stub isn't within the
        // `wait_for_stop_reason` callback.
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    SimpleLogger::new().env().init().unwrap();

    let mut emulator = SimpleEmulator::new(include_bytes!("test_program/test.bin"));

    let conn: Box<dyn ConnectionExt<Error = std::io::Error>> = Box::new(wait_for_connection(1337)?);
    let gdb = GdbStub::new(conn);
    let result = gdb.run_blocking::<SimpleEmulator>(&mut emulator.cpu);

    info!("emulator stopped, gdb result {:?}", result);

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use arm7tdmi::memory::DebugRead;
    use rustboyadvance_utils::elf::read_symbols;

    #[test]
    fn test_breakpoint() -> Result<(), Box<dyn std::error::Error>> {
        let mut emulator = SimpleEmulator::new(include_bytes!("test_program/test.bin"));
        let symbol_map = read_symbols(include_bytes!("test_program/test.elf"))?;
        let breakpoint_addr = *symbol_map.get("breakpoint_on_me").unwrap();
        println!("breakpoint_addr = {:08x}", breakpoint_addr);
        let breakpoint_counter_addr = *symbol_map.get("breakpoint_count").unwrap();
        emulator.cpu.breakpoints.push(breakpoint_addr);

        for x in 0..10 {
            println!("{}", x);
            let timeout = std::time::Instant::now() + std::time::Duration::from_secs(1);
            loop {
                emulator.cpu.step();
                if let Some(addr) = emulator.cpu.check_breakpoint() {
                    emulator.cpu.step();
                    assert_eq!(addr, breakpoint_addr);
                    assert_eq!(emulator.cpu.bus.debug_read_32(breakpoint_counter_addr), x);
                    break;
                }
                assert!(std::time::Instant::now() < timeout);
            }
        }

        Ok(())
    }
}
