use std::time::Duration;

use arm7tdmi::gdb::gdbstub::{
    conn::{Connection, ConnectionExt},
    stub::{run_blocking, SingleThreadStopReason},
    target::Target,
};

use super::{target::DebuggerTarget, DebuggerRequest};

pub(crate) struct DebuggerEventLoop {}

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
                if let Some(stop_reason) =
                    target.wait_for_stop_reason_timeout(Duration::from_millis(10))
                {
                    info!("Target stopped due to {:?}!", stop_reason);
                    return Ok(run_blocking::Event::TargetStopped(stop_reason));
                }
            }
        }
    }

    fn on_interrupt(
        target: &mut DebuggerTarget,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <DebuggerTarget as Target>::Error> {
        info!("on_interrupt: sending stop message");
        target.debugger_request(DebuggerRequest::Interrupt);
        info!("Waiting for target to stop <blocking>");
        Ok(Some(target.wait_for_stop_reason_blocking()))
    }
}
