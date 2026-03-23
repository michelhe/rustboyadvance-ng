use serde::{Deserialize, Serialize};

/// Duty cycle lookup table: each entry is 8 steps of high/low for the square wave.
/// Index by duty (0-3), then by position (0-7). true = high output.
const DUTY_TABLE: [[bool; 8]; 4] = [
    [false, false, false, false, false, false, false, true],  // 12.5%
    [true, false, false, false, false, false, false, true],   // 25%
    [true, false, false, false, false, true, true, true],     // 50%
    [false, true, true, true, true, true, true, false],       // 75%
];

/// CPU cycles per frame sequencer step (512 Hz from 16.78 MHz CPU).
const FRAME_SEQUENCER_PERIOD: u32 = 32768;

// ─── Frame Sequencer ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FrameSequencer {
    step: u8,
    cycle_counter: u32,
}

impl Default for FrameSequencer {
    fn default() -> Self {
        Self {
            step: 0,
            cycle_counter: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FrameSequencerClocks {
    length: bool,
    envelope: bool,
    sweep: bool,
}

impl FrameSequencer {
    fn tick(&mut self, cycles: u32) -> FrameSequencerClocks {
        self.cycle_counter += cycles;
        if self.cycle_counter >= FRAME_SEQUENCER_PERIOD {
            self.cycle_counter -= FRAME_SEQUENCER_PERIOD;
            let step = self.step;
            self.step = (self.step + 1) % 8;
            FrameSequencerClocks {
                length: step % 2 == 0,          // steps 0, 2, 4, 6
                sweep: step == 2 || step == 6,  // steps 2, 6
                envelope: step == 7,            // step 7
            }
        } else {
            FrameSequencerClocks {
                length: false,
                envelope: false,
                sweep: false,
            }
        }
    }
}

// ─── Length Counter ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct LengthCounter {
    enabled: bool,
    counter: u16,
    max: u16,
}

impl LengthCounter {
    fn new(max: u16) -> Self {
        Self {
            enabled: false,
            counter: 0,
            max,
        }
    }

    /// Returns true if the channel should be disabled.
    fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                return true;
            }
        }
        false
    }
}

// ─── Envelope ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Envelope {
    initial_volume: u8,
    current_volume: u8,
    direction: bool, // true = increase
    period: u8,      // 0 = disabled
    timer: u8,
}

impl Envelope {
    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = self.period;
            if self.direction && self.current_volume < 15 {
                self.current_volume += 1;
            } else if !self.direction && self.current_volume > 0 {
                self.current_volume -= 1;
            }
        }
    }

    fn trigger(&mut self) {
        self.current_volume = self.initial_volume;
        self.timer = self.period;
    }

    fn write(&mut self, value: u16) {
        self.period = (value & 0x7) as u8;
        self.direction = (value >> 3) & 1 != 0;
        self.initial_volume = ((value >> 4) & 0xf) as u8;
    }

    /// DAC is off when initial volume is 0 and direction is decrease.
    fn dac_enabled(&self) -> bool {
        self.initial_volume != 0 || self.direction
    }
}

// ─── Sweep (Channel 1 only) ────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Sweep {
    shift: u8,
    direction: bool, // true = subtract
    period: u8,      // 0 = disabled
    timer: u8,
    shadow_freq: u16,
    enabled: bool,
}

impl Sweep {
    /// Returns Some(new_freq) if overflow (>2047), meaning channel should be disabled.
    /// Returns None if no overflow.
    fn calculate_freq(&self) -> (u16, bool) {
        let delta = self.shadow_freq >> self.shift;
        let new_freq = if self.direction {
            self.shadow_freq.wrapping_sub(delta)
        } else {
            self.shadow_freq + delta
        };
        (new_freq, new_freq > 2047)
    }

    /// Clock the sweep unit. Returns (new_frequency_or_None, should_disable).
    fn clock(&mut self) -> (Option<u16>, bool) {
        if !self.enabled || self.period == 0 {
            return (None, false);
        }
        if self.timer > 0 {
            self.timer -= 1;
        }
        if self.timer == 0 {
            self.timer = if self.period != 0 { self.period } else { 8 };
            let (new_freq, overflow) = self.calculate_freq();
            if overflow {
                return (None, true);
            }
            if self.shift != 0 {
                self.shadow_freq = new_freq;
                // Do overflow check again with new frequency
                let (_, overflow2) = self.calculate_freq();
                if overflow2 {
                    return (Some(new_freq), true);
                }
                return (Some(new_freq), false);
            }
        }
        (None, false)
    }

    fn trigger(&mut self, frequency: u16) {
        self.shadow_freq = frequency;
        self.timer = if self.period != 0 { self.period } else { 8 };
        self.enabled = self.period != 0 || self.shift != 0;
        // If shift is nonzero, do an overflow check immediately
        if self.shift != 0 {
            let (_, overflow) = self.calculate_freq();
            if overflow {
                self.enabled = false;
            }
        }
    }

    fn write(&mut self, value: u16) {
        self.shift = (value & 0x7) as u8;
        self.direction = (value >> 3) & 1 != 0;
        self.period = ((value >> 4) & 0x7) as u8;
    }
}

// ─── Channel 1: Square + Sweep ──────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PsgChannel1 {
    enabled: bool,
    duty: u8,
    frequency: u16,
    duty_pos: u8,
    timer: i32,
    sweep: Sweep,
    envelope: Envelope,
    length: LengthCounter,
}

impl Default for PsgChannel1 {
    fn default() -> Self {
        Self {
            enabled: false,
            duty: 0,
            frequency: 0,
            duty_pos: 0,
            timer: 0,
            sweep: Sweep::default(),
            envelope: Envelope::default(),
            length: LengthCounter::new(64),
        }
    }
}

impl PsgChannel1 {
    fn period(&self) -> i32 {
        (2048 - self.frequency as i32) * 16
    }

    fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            self.duty_pos = (self.duty_pos + 1) % 8;
        }
    }

    fn sample(&self) -> i16 {
        if !self.enabled {
            return 0;
        }
        if DUTY_TABLE[self.duty as usize][self.duty_pos as usize] {
            self.envelope.current_volume as i16
        } else {
            0
        }
    }

    fn clock_length(&mut self) {
        if self.length.clock() {
            self.enabled = false;
        }
    }

    fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    fn clock_sweep(&mut self) {
        let (new_freq, disable) = self.sweep.clock();
        if disable {
            self.enabled = false;
        }
        if let Some(freq) = new_freq {
            self.frequency = freq;
        }
    }

    pub fn write_sweep(&mut self, value: u16) {
        self.sweep.write(value);
    }

    pub fn read_sweep(&self) -> u16 {
        (self.sweep.shift as u16)
            | ((self.sweep.direction as u16) << 3)
            | ((self.sweep.period as u16) << 4)
    }

    pub fn write_duty_envelope(&mut self, value: u16) {
        self.length.counter = 64 - (value & 0x3f) as u16;
        self.duty = ((value >> 6) & 0x3) as u8;
        self.envelope.write(value >> 8);
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }

    pub fn read_duty_envelope(&self) -> u16 {
        // Length is write-only; duty and envelope are readable
        ((self.duty as u16) << 6)
            | ((self.envelope.period as u16) << 8)
            | ((self.envelope.direction as u16) << 11)
            | ((self.envelope.initial_volume as u16) << 12)
    }

    pub fn write_freq_control(&mut self, value: u16) {
        self.frequency = value & 0x7ff;
        self.length.enabled = (value >> 14) & 1 != 0;
        if (value >> 15) & 1 != 0 {
            self.trigger();
        }
    }

    pub fn read_freq_control(&self) -> u16 {
        // Frequency is write-only; only length enable (bit 14) is readable
        (self.length.enabled as u16) << 14
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length.counter == 0 {
            self.length.counter = 64;
        }
        self.timer = self.period();
        self.envelope.trigger();
        self.sweep.trigger(self.frequency);
        if !self.sweep.enabled && (self.sweep.period != 0 || self.sweep.shift != 0) {
            // sweep trigger may have disabled us
        }
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }
}

// ─── Channel 2: Square (no sweep) ──────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PsgChannel2 {
    enabled: bool,
    duty: u8,
    frequency: u16,
    duty_pos: u8,
    timer: i32,
    envelope: Envelope,
    length: LengthCounter,
}

impl Default for PsgChannel2 {
    fn default() -> Self {
        Self {
            enabled: false,
            duty: 0,
            frequency: 0,
            duty_pos: 0,
            timer: 0,
            envelope: Envelope::default(),
            length: LengthCounter::new(64),
        }
    }
}

impl PsgChannel2 {
    fn period(&self) -> i32 {
        (2048 - self.frequency as i32) * 16
    }

    fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            self.duty_pos = (self.duty_pos + 1) % 8;
        }
    }

    fn sample(&self) -> i16 {
        if !self.enabled {
            return 0;
        }
        if DUTY_TABLE[self.duty as usize][self.duty_pos as usize] {
            self.envelope.current_volume as i16
        } else {
            0
        }
    }

    fn clock_length(&mut self) {
        if self.length.clock() {
            self.enabled = false;
        }
    }

    fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub fn write_duty_envelope(&mut self, value: u16) {
        self.length.counter = 64 - (value & 0x3f) as u16;
        self.duty = ((value >> 6) & 0x3) as u8;
        self.envelope.write(value >> 8);
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }

    pub fn read_duty_envelope(&self) -> u16 {
        ((self.duty as u16) << 6)
            | ((self.envelope.period as u16) << 8)
            | ((self.envelope.direction as u16) << 11)
            | ((self.envelope.initial_volume as u16) << 12)
    }

    pub fn write_freq_control(&mut self, value: u16) {
        self.frequency = value & 0x7ff;
        self.length.enabled = (value >> 14) & 1 != 0;
        if (value >> 15) & 1 != 0 {
            self.trigger();
        }
    }

    pub fn read_freq_control(&self) -> u16 {
        (self.length.enabled as u16) << 14
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length.counter == 0 {
            self.length.counter = 64;
        }
        self.timer = self.period();
        self.envelope.trigger();
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }
}

// ─── Channel 3: Wave ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PsgChannel3 {
    enabled: bool,
    dac_enabled: bool,
    frequency: u16,
    volume_code: u8,
    force_volume: bool,
    sample_pos: u8,
    timer: i32,
    length: LengthCounter,
    /// Two banks of 16 bytes each (GBA has two wave RAM banks)
    wave_ram: [u8; 32],
    /// false = single bank mode, true = double bank mode
    bank_mode: bool,
    /// Which bank is used for playback (0 or 1)
    bank_select: u8,
}

impl Default for PsgChannel3 {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            frequency: 0,
            volume_code: 0,
            force_volume: false,
            sample_pos: 0,
            timer: 0,
            length: LengthCounter::new(256),
            wave_ram: [0; 32],
            bank_mode: false,
            bank_select: 0,
        }
    }
}

impl PsgChannel3 {
    fn period(&self) -> i32 {
        (2048 - self.frequency as i32) * 8
    }

    fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            let num_samples = if self.bank_mode { 32 } else { 64 };
            self.sample_pos = (self.sample_pos + 1) % num_samples;
        }
    }

    fn sample(&self) -> i16 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }

        // Determine which byte and nibble to read
        let (bank_offset, pos) = if self.bank_mode {
            // Two-bank mode: play from the selected bank
            (self.bank_select as usize * 16, self.sample_pos)
        } else {
            // Single bank mode: play all 64 samples (both banks sequentially)
            (0, self.sample_pos)
        };

        let byte_idx = bank_offset + (pos / 2) as usize;
        let byte = self.wave_ram[byte_idx];
        let nibble = if pos % 2 == 0 {
            (byte >> 4) & 0xf
        } else {
            byte & 0xf
        };

        // Apply volume
        let shifted = if self.force_volume {
            // Force 75% volume (shift right by 1, then multiply by 3/4... actually just 3/4)
            (nibble * 3) / 4
        } else {
            match self.volume_code {
                0 => 0,
                1 => nibble,
                2 => nibble >> 1,
                3 => nibble >> 2,
                _ => unreachable!(),
            }
        };
        shifted as i16
    }

    fn clock_length(&mut self) {
        if self.length.clock() {
            self.enabled = false;
        }
    }

    pub fn write_bank_control(&mut self, value: u16) {
        self.bank_mode = (value >> 5) & 1 != 0;
        self.bank_select = ((value >> 6) & 1) as u8;
        self.dac_enabled = (value >> 7) & 1 != 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    pub fn read_bank_control(&self) -> u16 {
        ((self.bank_mode as u16) << 5)
            | ((self.bank_select as u16) << 6)
            | ((self.dac_enabled as u16) << 7)
    }

    pub fn write_length_volume(&mut self, value: u16) {
        self.length.counter = 256 - (value & 0xff) as u16;
        self.volume_code = ((value >> 13) & 0x3) as u8;
        self.force_volume = (value >> 15) & 1 != 0;
    }

    pub fn read_length_volume(&self) -> u16 {
        // Length is write-only
        ((self.volume_code as u16) << 13) | ((self.force_volume as u16) << 15)
    }

    pub fn write_freq_control(&mut self, value: u16) {
        self.frequency = value & 0x7ff;
        self.length.enabled = (value >> 14) & 1 != 0;
        if (value >> 15) & 1 != 0 {
            self.trigger();
        }
    }

    pub fn read_freq_control(&self) -> u16 {
        (self.length.enabled as u16) << 14
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length.counter == 0 {
            self.length.counter = 256;
        }
        self.timer = self.period();
        self.sample_pos = 0;
        if !self.dac_enabled {
            self.enabled = false;
        }
    }

    /// Write a byte to wave RAM. Offset is 0x00..0x0F.
    /// Writes go to the bank NOT currently selected for playback.
    pub fn write_wave_ram(&mut self, offset: usize, value: u8) {
        let bank = if self.bank_mode {
            // In two-bank mode, writes go to the non-playing bank
            (1 - self.bank_select) as usize
        } else {
            // In single-bank mode, writes go to the currently selected bank
            self.bank_select as usize
        };
        let idx = bank * 16 + offset;
        if idx < 32 {
            self.wave_ram[idx] = value;
        }
    }

    /// Read a byte from wave RAM. Offset is 0x00..0x0F.
    pub fn read_wave_ram(&self, offset: usize) -> u8 {
        let bank = if self.bank_mode {
            (1 - self.bank_select) as usize
        } else {
            self.bank_select as usize
        };
        let idx = bank * 16 + offset;
        if idx < 32 {
            self.wave_ram[idx]
        } else {
            0
        }
    }
}

// ─── Channel 4: Noise ───────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PsgChannel4 {
    enabled: bool,
    dividing_ratio: u8,
    width_mode: bool, // true = 7-bit, false = 15-bit
    shift_clock: u8,
    lfsr: u16,
    timer: i32,
    envelope: Envelope,
    length: LengthCounter,
}

impl Default for PsgChannel4 {
    fn default() -> Self {
        Self {
            enabled: false,
            dividing_ratio: 0,
            width_mode: false,
            shift_clock: 0,
            lfsr: 0x7fff,
            timer: 0,
            envelope: Envelope::default(),
            length: LengthCounter::new(64),
        }
    }
}

impl PsgChannel4 {
    fn period(&self) -> i32 {
        let r = self.dividing_ratio as i32;
        let divisor = if r == 0 { 8 } else { r * 16 };
        divisor << (self.shift_clock as i32 + 1)
    }

    fn tick(&mut self, cycles: u32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            self.clock_lfsr();
        }
    }

    fn clock_lfsr(&mut self) {
        let xor_bit = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
        self.lfsr >>= 1;
        self.lfsr |= xor_bit << 14;
        if self.width_mode {
            // 7-bit mode: also set bit 6
            self.lfsr = (self.lfsr & !0x40) | (xor_bit << 6);
        }
    }

    fn sample(&self) -> i16 {
        if !self.enabled {
            return 0;
        }
        // Output is inverted bit 0 of LFSR
        if self.lfsr & 1 == 0 {
            self.envelope.current_volume as i16
        } else {
            0
        }
    }

    fn clock_length(&mut self) {
        if self.length.clock() {
            self.enabled = false;
        }
    }

    fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub fn write_length_envelope(&mut self, value: u16) {
        self.length.counter = 64 - (value & 0x3f) as u16;
        self.envelope.write(value >> 8);
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }

    pub fn read_length_envelope(&self) -> u16 {
        // Length is write-only
        ((self.envelope.period as u16) << 8)
            | ((self.envelope.direction as u16) << 11)
            | ((self.envelope.initial_volume as u16) << 12)
    }

    pub fn write_freq_control(&mut self, value: u16) {
        self.dividing_ratio = (value & 0x7) as u8;
        self.width_mode = (value >> 3) & 1 != 0;
        self.shift_clock = ((value >> 4) & 0xf) as u8;
        self.length.enabled = (value >> 14) & 1 != 0;
        if (value >> 15) & 1 != 0 {
            self.trigger();
        }
    }

    pub fn read_freq_control(&self) -> u16 {
        (self.dividing_ratio as u16)
            | ((self.width_mode as u16) << 3)
            | ((self.shift_clock as u16) << 4)
            | ((self.length.enabled as u16) << 14)
    }

    fn trigger(&mut self) {
        self.enabled = true;
        if self.length.counter == 0 {
            self.length.counter = 64;
        }
        self.timer = self.period();
        self.lfsr = 0x7fff;
        self.envelope.trigger();
        if !self.envelope.dac_enabled() {
            self.enabled = false;
        }
    }
}

// ─── PSG Mixer ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Psg {
    frame_sequencer: FrameSequencer,
    pub channel1: PsgChannel1,
    pub channel2: PsgChannel2,
    pub channel3: PsgChannel3,
    pub channel4: PsgChannel4,
}

impl Psg {
    /// Advance all PSG state by the given number of CPU cycles.
    pub fn tick(&mut self, cycles: u32) {
        let clocks = self.frame_sequencer.tick(cycles);

        if clocks.length {
            self.channel1.clock_length();
            self.channel2.clock_length();
            self.channel3.clock_length();
            self.channel4.clock_length();
        }
        if clocks.sweep {
            self.channel1.clock_sweep();
        }
        if clocks.envelope {
            self.channel1.clock_envelope();
            self.channel2.clock_envelope();
            self.channel4.clock_envelope();
        }

        self.channel1.tick(cycles);
        self.channel2.tick(cycles);
        self.channel3.tick(cycles);
        self.channel4.tick(cycles);
    }

    /// Get the mixed PSG sample for one stereo channel.
    ///
    /// `stereo_channel`: 0 = left, 1 = right
    /// `enable_flags`: [ch1_enabled, ch2_enabled, ch3_enabled, ch4_enabled] for this side
    /// `master_volume`: 0-7 from SOUNDCNT_L
    pub fn sample(&self, enable_flags: [bool; 4], master_volume: usize) -> i16 {
        let mut sum: i16 = 0;
        if enable_flags[0] {
            sum += self.channel1.sample();
        }
        if enable_flags[1] {
            sum += self.channel2.sample();
        }
        if enable_flags[2] {
            sum += self.channel3.sample();
        }
        if enable_flags[3] {
            sum += self.channel4.sample();
        }
        // Scale by master volume (1-8)
        sum * (1 + master_volume as i16) / 8
    }

    /// Returns which channels are currently active (bits 0-3).
    pub fn status_bits(&self) -> u16 {
        (self.channel1.enabled as u16)
            | ((self.channel2.enabled as u16) << 1)
            | ((self.channel3.enabled as u16) << 2)
            | ((self.channel4.enabled as u16) << 3)
    }
}
