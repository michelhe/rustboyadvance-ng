// TODO write tests or replace with a crate
const SOUND_FIFO_CAPACITY: usize = 32;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SoundFifo {
    wr_pos: usize,
    rd_pos: usize,
    count: usize,
    data: [i8; SOUND_FIFO_CAPACITY],
}

impl SoundFifo {
    pub fn new() -> SoundFifo {
        SoundFifo {
            wr_pos: 0,
            rd_pos: 0,
            count: 0,
            data: [0; SOUND_FIFO_CAPACITY],
        }
    }

    pub fn write(&mut self, value: i8) {
        if self.count >= SOUND_FIFO_CAPACITY {
            return;
        }
        self.data[self.wr_pos] = value;
        self.wr_pos = (self.wr_pos + 1) % SOUND_FIFO_CAPACITY;
        self.count += 1;
    }

    pub fn read(&mut self) -> i8 {
        if self.count == 0 {
            return 0;
        };
        let value = self.data[self.rd_pos];
        self.rd_pos = (self.rd_pos + 1) % SOUND_FIFO_CAPACITY;
        self.count -= 1;
        value
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn reset(&mut self) {
        self.wr_pos = 0;
        self.rd_pos = 0;
        self.count = 0;
    }
}
