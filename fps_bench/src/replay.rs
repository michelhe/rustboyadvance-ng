//! Reader side of the SDL-frontend input recorder (see
//! `platform/rustboyadvance-sdl2/src/recorder.rs` for the writer and file
//! format description).
//!
//! Loads the whole recording into memory at startup (a full minute of live
//! play is typically < 10 KB — edges are sparse) and exposes `apply_due`
//! to hand the replayer the key-state edges it should apply before the next
//! `gba.frame()` given the current emulated cycle count.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const MAGIC: &[u8; 8] = b"RBAREC01";

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub cycle: u64,
    pub state: u16,
}

pub struct Replayer {
    events: Vec<Event>,
    cursor: usize,
}

impl Replayer {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let mut file = BufReader::new(File::open(path)?);

        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("input recording magic mismatch: got {:?}", magic),
            ));
        }

        // Read events until EOF. Each record is exactly 10 bytes.
        let mut events = Vec::new();
        let mut buf = [0u8; 10];
        loop {
            match file.read_exact(&mut buf) {
                Ok(()) => {
                    let cycle = u64::from_le_bytes(buf[0..8].try_into().unwrap());
                    let state = u16::from_le_bytes(buf[8..10].try_into().unwrap());
                    events.push(Event { cycle, state });
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }

        Ok(Replayer { events, cursor: 0 })
    }

    /// Number of events in the recording.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Cycle stamp of the last recorded event. Replay callers stop the
    /// benchmark once `gba.cycles() >= last_cycle()`, guaranteeing both CPU
    /// builds do the same total emulated work.
    pub fn last_cycle(&self) -> u64 {
        self.events.last().map(|e| e.cycle).unwrap_or(0)
    }

    /// Apply every pending event whose cycle is already behind the emulator,
    /// writing each event's keystate into `*key_state` in chronological
    /// order. The final write "wins" per frame — that matches the recorder,
    /// which samples once at end-of-frame.
    pub fn apply_due(&mut self, now: u64, key_state: &mut u16) {
        while self.cursor < self.events.len() && self.events[self.cursor].cycle <= now {
            *key_state = self.events[self.cursor].state;
            self.cursor += 1;
        }
    }

    /// True once every recorded edge has been handed to the replayer.
    pub fn exhausted(&self) -> bool {
        self.cursor >= self.events.len()
    }
}
