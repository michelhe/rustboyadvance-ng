//! Input recorder for the SDL frontend.
//!
//! Each time the GBA keypad bitmask changes, we append one record of
//! `(emulated_cycle_count, new_keypad_state)` to a binary file. The replay
//! harness (fps_bench --replay) reads the file back and drives the emulator's
//! key state from these recorded edges in emulated time, producing a
//! deterministic input trace that both interpreter variants run identically.
//!
//! File format (little-endian):
//!   offset 0:  magic "RBAREC01"              (8 bytes)
//!   offset 8:  repeated records, each:
//!              cycle:u64  keystate:u16       (10 bytes)
//!
//! Records are kept chronologically by construction (each write happens when
//! the state actually changes, at the current emulator cycle count).
//!
//! An implicit "end-of-recording" marker is the final record written at
//! recorder shutdown with the *current* keystate; the replayer uses the last
//! record's cycle as the stop timestamp.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"RBAREC01";

pub struct Recorder {
    file: BufWriter<File>,
    last_state: u16,
}

impl Recorder {
    /// Create a new recording file. Writes the magic header and an initial
    /// record at cycle 0 reflecting `initial_state`, so the replayer starts
    /// from a consistent keypad state regardless of what the game's own
    /// initialization code does.
    pub fn create(path: &Path, initial_state: u16) -> std::io::Result<Self> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(MAGIC)?;
        // Initial record — start at cycle 0.
        Self::write_record(&mut file, 0, initial_state)?;
        Ok(Recorder { file, last_state: initial_state })
    }

    /// Record a keypad-state change if the state actually differs from the
    /// previous write. No-op if unchanged.
    pub fn observe(&mut self, cycles: usize, state: u16) -> std::io::Result<()> {
        if state == self.last_state {
            return Ok(());
        }
        Self::write_record(&mut self.file, cycles as u64, state)?;
        self.last_state = state;
        Ok(())
    }

    /// Flush the underlying writer. Called at recorder drop to make sure the
    /// buffered events are on disk even if the process exits abruptly.
    #[allow(dead_code)] // public helper; Drop already flushes but callers can force it earlier
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }

    fn write_record(w: &mut BufWriter<File>, cycles: u64, state: u16) -> std::io::Result<()> {
        w.write_all(&cycles.to_le_bytes())?;
        w.write_all(&state.to_le_bytes())?;
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Best-effort flush so the benchmark replayer sees the full trace.
        let _ = self.file.flush();
    }
}
