use std::time::Duration;

use arm7tdmi::gdb::gdbstub::{
    conn::{Connection, ConnectionExt},
    stub::{run_blocking, SingleThreadStopReason},
    target::Target,
};

use super::{DebuggerRequest, DebuggerTarget};

pub struct DebuggerEventLoop {}

impl run_blocking::BlockingEventLoop for DebuggerEventLoop {
    type Target = DebuggerTarget;
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
        let mut poll_incoming_data = || conn.peek().map(|b| b.is_some()).unwrap_or(true);

        loop {
            if poll_incoming_data() {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                return Ok(run_blocking::Event::IncomingData(byte));
            } else {
                // try and wait for the stop reason
                let (lock, cvar) = &*target.stop_signal;
                let stop_reason = lock.lock().unwrap();
                let (stop_reason, timeout_result) = cvar
                    .wait_timeout(stop_reason, Duration::from_millis(10))
                    .unwrap();
                if timeout_result.timed_out() {
                    // timed-out, try again later
                    continue;
                }
                info!("Target stopped due to {:?}!", stop_reason);
                return Ok(run_blocking::Event::TargetStopped(*stop_reason));
            }
        }
    }

    fn on_interrupt(
        target: &mut DebuggerTarget,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <DebuggerTarget as Target>::Error> {
        info!("on_interrupt: sending stop message");
        target.tx.send(DebuggerRequest::Interrupt).unwrap();
        target.wait_for_operation();
        info!("Waiting for target to stop <blocking>");
        let (lock, cvar) = &*target.stop_signal;
        let stop_signal = lock.lock().unwrap();
        let stop_signal = cvar.wait(stop_signal).unwrap();
        Ok(Some(*stop_signal))
    }
}
