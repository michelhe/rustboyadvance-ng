use arm7tdmi::gdb::gdbstub::{
    common::Signal,
    conn::{Connection, ConnectionExt},
    stub::{run_blocking, SingleThreadStopReason},
    target::Target,
};

use crate::GameBoyAdvance;

pub struct GbaGdbEventLoop {}

impl run_blocking::BlockingEventLoop for GbaGdbEventLoop {
    type Target = GameBoyAdvance;
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
        let mut poll_incoming_data = || {
            // gdbstub takes ownership of the underlying connection, so the `borrow_conn`
            // method is used to borrow the underlying connection back from the stub to
            // check for incoming data.
            conn.peek().map(|b| b.is_some()).unwrap_or(true)
        };

        loop {
            if poll_incoming_data() {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                return Ok(run_blocking::Event::IncomingData(byte));
            } else {
                target.frame_and_check_breakpoints();
                if target.cpu.check_breakpoint().is_some() {
                    return Ok(run_blocking::Event::TargetStopped(
                        SingleThreadStopReason::SwBreak(()),
                    ));
                }
            }
        }
    }

    fn on_interrupt(
        _target: &mut GameBoyAdvance,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <GameBoyAdvance as Target>::Error> {
        // Because this emulator runs as part of the GDB stub loop, there isn't any
        // special action that needs to be taken to interrupt the underlying target. It
        // is implicitly paused whenever the stub isn't within the
        // `wait_for_stop_reason` callback.
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}
