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

    /// Returns true if overflow was detected (channel should be disabled).
    fn trigger(&mut self, frequency: u16) -> bool {
        self.shadow_freq = frequency;
        self.timer = if self.period != 0 { self.period } else { 8 };
        self.enabled = self.period != 0 || self.shift != 0;
        // If shift is nonzero, do an overflow check immediately
        if self.shift != 0 {
            let (_, overflow) = self.calculate_freq();
            if overflow {
                self.enabled = false;
                return true;
            }
        }
        false
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
        if self.sweep.trigger(self.frequency) {
            self.enabled = false;
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
            let num_samples: u8 = if self.bank_mode { 64 } else { 32 };
            self.sample_pos = (self.sample_pos + 1) % num_samples;
        }
    }

    fn sample(&self) -> i16 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }

        // Determine which byte and nibble to read.
        // In single-bank mode (bank_mode=false): play 32 samples from the non-selected bank.
        // In two-bank mode (bank_mode=true): play 64 samples starting from selected bank.
        let start_byte = if self.bank_mode {
            self.bank_select as usize * 16
        } else {
            (1 - self.bank_select) as usize * 16
        };

        let byte_idx = (start_byte + (self.sample_pos / 2) as usize) % 32;
        let byte = self.wave_ram[byte_idx];
        let nibble = if self.sample_pos % 2 == 0 {
            (byte >> 4) & 0xf
        } else {
            byte & 0xf
        };

        // Apply volume shift
        let shifted = if self.force_volume {
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
        divisor << (self.shift_clock as i32 + 2)
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

    /// Get the raw mixed PSG sample for one stereo channel.
    /// Returns the unsigned sum of enabled channels (range 0-60).
    /// Master volume and DMG ratio scaling are applied by the caller.
    pub fn sample(&self, enable_flags: [bool; 4]) -> i16 {
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
        sum
    }

    /// Returns which channels are currently active (bits 0-3).
    pub fn status_bits(&self) -> u16 {
        (self.channel1.enabled as u16)
            | ((self.channel2.enabled as u16) << 1)
            | ((self.channel3.enabled as u16) << 2)
            | ((self.channel4.enabled as u16) << 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect N samples from a channel 1 by ticking at 512 cpu-cycles per sample.
    fn collect_ch1_samples(ch: &mut PsgChannel1, n: usize) -> Vec<i16> {
        (0..n)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect()
    }

    /// Count transitions (from 0→vol or vol→0) in a sample buffer.
    fn count_transitions(samples: &[i16]) -> usize {
        samples
            .windows(2)
            .filter(|w| (w[0] == 0) != (w[1] == 0))
            .count()
    }

    // ─── Channel 1: Square wave tests ───────────────────────────

    #[test]
    fn ch1_trigger_enables_channel() {
        let mut ch = PsgChannel1::default();
        assert!(!ch.enabled);

        // Set envelope with volume > 0 so DAC is on
        // REG_SOUND1CNT_H: length=0, duty=2 (50%), envelope: period=0, dir=0, vol=15
        ch.write_duty_envelope(0xF080); // vol=15, dir=0, period=0, duty=2
        // REG_SOUND1CNT_X: freq=1024, trigger=1
        ch.write_freq_control(0x8400); // bit15=trigger, freq=1024
        assert!(ch.enabled);
    }

    #[test]
    fn ch1_no_trigger_without_dac() {
        let mut ch = PsgChannel1::default();
        // Set envelope vol=0, dir=decrease → DAC disabled
        ch.write_duty_envelope(0x0080); // vol=0, dir=0, duty=2
        ch.write_freq_control(0x8400);
        assert!(!ch.enabled, "channel should not enable when DAC is off");
    }

    #[test]
    fn ch1_50pct_duty_produces_square_wave() {
        let mut ch = PsgChannel1::default();
        // duty=2 (50%), vol=15
        ch.write_duty_envelope(0xF080); // vol=15, duty=2
        // freq=1024 → period = (2048-1024)*16 = 16384 cpu cycles per full cycle
        // At 512 cycles/sample → 32 samples per cycle
        ch.write_freq_control(0x8400); // trigger, freq=1024

        // Collect enough samples for several cycles
        let samples = collect_ch1_samples(&mut ch, 256);

        // With 50% duty, roughly half should be nonzero
        let nonzero = samples.iter().filter(|&&s| s > 0).count();
        let ratio = nonzero as f64 / samples.len() as f64;
        assert!(
            (0.40..=0.60).contains(&ratio),
            "50% duty should have ~50% high samples, got {:.1}%",
            ratio * 100.0
        );

        // Verify amplitude is correct (vol=15)
        let max_val = *samples.iter().max().unwrap();
        assert_eq!(max_val, 15, "max sample should equal envelope volume");
    }

    #[test]
    fn ch1_125pct_duty() {
        let mut ch = PsgChannel1::default();
        // duty=0 (12.5%), vol=15
        ch.write_duty_envelope(0xF000); // vol=15, duty=0
        ch.write_freq_control(0x8400); // trigger, freq=1024

        let samples = collect_ch1_samples(&mut ch, 512);
        let nonzero = samples.iter().filter(|&&s| s > 0).count();
        let ratio = nonzero as f64 / samples.len() as f64;
        assert!(
            (0.08..=0.20).contains(&ratio),
            "12.5% duty should have ~12.5% high samples, got {:.1}%",
            ratio * 100.0
        );
    }

    #[test]
    fn ch1_frequency_affects_pitch() {
        // Higher frequency value → shorter period → more transitions per sample window
        let mut ch_low = PsgChannel1::default();
        ch_low.write_duty_envelope(0xF080);
        ch_low.write_freq_control(0x8000 | 512); // freq=512

        let mut ch_high = PsgChannel1::default();
        ch_high.write_duty_envelope(0xF080);
        ch_high.write_freq_control(0x8000 | 1536); // freq=1536

        let samples_low = collect_ch1_samples(&mut ch_low, 1024);
        let samples_high = collect_ch1_samples(&mut ch_high, 1024);

        let transitions_low = count_transitions(&samples_low);
        let transitions_high = count_transitions(&samples_high);

        assert!(
            transitions_high > transitions_low * 2,
            "higher freq should have more transitions: low={}, high={}",
            transitions_low,
            transitions_high
        );
    }

    #[test]
    fn ch1_length_counter_disables_channel() {
        let mut ch = PsgChannel1::default();
        // Set length=63 → counter = 64 - 63 = 1
        ch.write_duty_envelope(0xF03F); // vol=15, duty=0, length=63
        ch.write_freq_control(0xC400); // trigger + length enable, freq=1024

        assert!(ch.enabled);
        // Clock length once → counter goes from 1 to 0 → disable
        ch.clock_length();
        assert!(!ch.enabled, "channel should be disabled after length expires");
    }

    #[test]
    fn ch1_envelope_decreases_volume() {
        let mut ch = PsgChannel1::default();
        // vol=15, dir=decrease, period=1
        ch.write_duty_envelope(0xF180);
        ch.write_freq_control(0x8400);

        assert_eq!(ch.envelope.current_volume, 15);

        // Clock envelope several times
        for expected in (0..15).rev() {
            ch.clock_envelope();
            assert_eq!(
                ch.envelope.current_volume, expected,
                "volume should decrease each envelope clock"
            );
        }
        // Should not go below 0
        ch.clock_envelope();
        assert_eq!(ch.envelope.current_volume, 0);
    }

    #[test]
    fn ch1_envelope_increases_volume() {
        let mut ch = PsgChannel1::default();
        // vol=0, dir=increase (bit 11), period=1
        ch.write_duty_envelope(0x0980);
        ch.write_freq_control(0x8400);

        assert_eq!(ch.envelope.current_volume, 0);
        ch.clock_envelope();
        assert_eq!(ch.envelope.current_volume, 1);
        ch.clock_envelope();
        assert_eq!(ch.envelope.current_volume, 2);
    }

    #[test]
    fn ch1_sweep_increases_frequency() {
        let mut ch = PsgChannel1::default();
        // sweep: shift=1, dir=increase (0), period=1
        ch.write_sweep(0x0011); // period=1, dir=0, shift=1
        ch.write_duty_envelope(0xF080);
        // Use freq=400 so double-overflow check doesn't trigger:
        // first: 400 + 200 = 600, second check: 600 + 300 = 900, both < 2047
        ch.write_freq_control(0x8000 | 400); // freq=400, trigger

        assert_eq!(ch.frequency, 400);
        ch.clock_sweep();
        assert_eq!(ch.frequency, 600);
        assert!(ch.enabled);
    }

    #[test]
    fn ch1_sweep_overflow_disables() {
        let mut ch = PsgChannel1::default();
        ch.write_sweep(0x0011); // period=1, shift=1, dir=increase
        ch.write_duty_envelope(0xF080);
        ch.write_freq_control(0x8000 | 2000); // freq=2000, trigger

        // sweep: 2000 + (2000 >> 1) = 3000 > 2047 → disable
        ch.clock_sweep();
        assert!(!ch.enabled, "sweep overflow should disable channel");
    }

    #[test]
    fn ch1_sweep_double_overflow_check() {
        let mut ch = PsgChannel1::default();
        ch.write_sweep(0x0011); // period=1, shift=1, dir=increase
        ch.write_duty_envelope(0xF080);
        // freq=1000: first calc 1000+500=1500 ok, but second calc 1500+750=2250 > 2047
        ch.write_freq_control(0x8000 | 1000);

        ch.clock_sweep();
        // Frequency was updated to 1500, but second overflow check disabled channel
        assert_eq!(ch.frequency, 1500);
        assert!(!ch.enabled, "double overflow check should disable channel");
    }

    // ─── Channel 2: Square wave (no sweep) ──────────────────────

    #[test]
    fn ch2_produces_output() {
        let mut ch = PsgChannel2::default();
        ch.write_duty_envelope(0xF080); // vol=15, duty=2
        ch.write_freq_control(0x8400); // trigger, freq=1024

        assert!(ch.enabled);

        let samples: Vec<i16> = (0..128)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect();

        let has_nonzero = samples.iter().any(|&s| s > 0);
        assert!(has_nonzero, "ch2 should produce nonzero samples");

        let max_val = *samples.iter().max().unwrap();
        assert_eq!(max_val, 15);
    }

    // ─── Channel 3: Wave ────────────────────────────────────────

    #[test]
    fn ch3_plays_wave_ram() {
        let mut ch = PsgChannel3::default();
        // Enable DAC, single bank mode, bank 1 selected (writes go to bank 1, plays from bank 0)
        // Wait — writes go to bank_select in single mode. Play comes from (1 - bank_select).
        // So select bank 1: writes go to bank 1, plays from bank 0.
        // We need to write to the PLAY bank. So select bank 0: writes to bank 0, plays from bank 1.
        // Hmm, that means our written data goes to bank 0 but plays from bank 1 (empty).
        // Instead, select bank 1: writes to bank 1, plays from bank 0 (empty). Still wrong.
        // The correct flow: write data, THEN switch bank_select so the written bank becomes playback.
        // Or: use two-bank mode where writes go to the non-playing bank.
        //
        // Simplest: write with bank_select=0 (data goes to bank 0), then switch to bank_select=1
        // (plays from bank 0).
        ch.write_bank_control(0x0080); // dac_enable, bank_select=0, single mode
        for i in 0..16 {
            ch.write_wave_ram(i, i as u8 * 17); // 0x00, 0x11, 0x22, ..., 0xFF → bank 0
        }
        // Switch to bank_select=1 so playback reads from bank 0 (where we wrote)
        ch.write_bank_control(0x00C0); // dac_enable + bank_select=1

        // volume=1 (100%), length doesn't matter
        ch.write_length_volume(0x2000); // vol_code=1 (bits 13-14)
        // freq=1800, trigger
        ch.write_freq_control(0x8000 | 1800);

        assert!(ch.enabled);

        // Collect samples
        let samples: Vec<i16> = (0..512)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect();

        let has_nonzero = samples.iter().any(|&s| s > 0);
        assert!(has_nonzero, "wave channel should produce nonzero output");

        // Check that we see a variety of values (not just 0 and max)
        let unique_values: std::collections::HashSet<i16> = samples.iter().cloned().collect();
        assert!(
            unique_values.len() > 2,
            "wave channel should produce varied output, got {} unique values",
            unique_values.len()
        );
    }

    #[test]
    fn ch3_volume_mutes_at_zero() {
        let mut ch = PsgChannel3::default();
        ch.write_bank_control(0x0080); // bank_select=0
        for i in 0..16 {
            ch.write_wave_ram(i, 0xFF); // writes to bank 0
        }
        ch.write_bank_control(0x00C0); // switch to bank_select=1, play from bank 0
        // volume=0 (mute)
        ch.write_length_volume(0x0000);
        ch.write_freq_control(0x8000 | 1800);

        let samples: Vec<i16> = (0..64)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect();

        assert!(
            samples.iter().all(|&s| s == 0),
            "wave channel at volume 0 should be silent"
        );
    }

    // ─── Channel 4: Noise ───────────────────────────────────────

    #[test]
    fn ch4_produces_pseudorandom_output() {
        let mut ch = PsgChannel4::default();
        // vol=15, period=0 (no envelope change)
        ch.write_length_envelope(0xF000);
        // divider=1, shift=2, 15-bit mode, trigger
        ch.write_freq_control(0x8021); // trigger, shift=2, div=1

        assert!(ch.enabled);

        let samples: Vec<i16> = (0..1024)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect();

        let nonzero = samples.iter().filter(|&&s| s > 0).count();
        let ratio = nonzero as f64 / samples.len() as f64;

        // LFSR should produce roughly 50% high/low for 15-bit mode
        assert!(
            (0.30..=0.70).contains(&ratio),
            "noise should be roughly 50/50, got {:.1}%",
            ratio * 100.0
        );
    }

    #[test]
    fn ch4_7bit_mode_repeats() {
        let mut ch = PsgChannel4::default();
        ch.write_length_envelope(0xF000);
        // 7-bit mode (bit 3), div=0, shift=0, trigger
        ch.write_freq_control(0x8008);

        // 7-bit LFSR has period of 127 (2^7 - 1)
        // Collect enough samples to see the pattern repeat
        // At div=0, shift=0: period = 8 << 2 = 32 cpu cycles per LFSR tick
        // At 512 cycles/sample: 512/32 = 16 LFSR ticks per sample
        // So 127 ticks takes about 8 samples. Collect plenty.
        let samples: Vec<i16> = (0..256)
            .map(|_| {
                ch.tick(512);
                ch.sample()
            })
            .collect();

        let has_nonzero = samples.iter().any(|&s| s > 0);
        assert!(has_nonzero, "7-bit noise should produce output");
    }

    // ─── Frame Sequencer ────────────────────────────────────────

    #[test]
    fn frame_sequencer_timing() {
        let mut fs = FrameSequencer::default();
        assert_eq!(fs.step, 0);

        // Should not advance before 32768 cycles
        let clocks = fs.tick(32767);
        assert!(!clocks.length);
        assert_eq!(fs.step, 0);

        // One more cycle should trigger step 0→1
        let clocks = fs.tick(1);
        assert!(clocks.length, "step 0 should clock length");
        assert!(!clocks.sweep, "step 0 should not clock sweep");
        assert!(!clocks.envelope, "step 0 should not clock envelope");
        assert_eq!(fs.step, 1);

        // Advance to step 2 (should clock length + sweep)
        let clocks = fs.tick(32768);
        assert_eq!(fs.step, 2);
        assert!(!clocks.length, "step 1 should not clock length");

        let clocks = fs.tick(32768);
        assert_eq!(fs.step, 3);
        assert!(clocks.length, "step 2 should clock length");
        assert!(clocks.sweep, "step 2 should clock sweep");

        // Advance to step 7 (should clock envelope)
        for _ in 3..7 {
            fs.tick(32768);
        }
        assert_eq!(fs.step, 7);
        let clocks = fs.tick(32768);
        assert!(clocks.envelope, "step 7 should clock envelope");
        assert_eq!(fs.step, 0, "should wrap back to 0");
    }

    // ─── Psg mixer ──────────────────────────────────────────────

    #[test]
    fn psg_mixer_produces_output() {
        let mut psg = Psg::default();

        // Configure channel 1: 50% duty, vol=15, freq=1024
        psg.channel1.write_duty_envelope(0xF080);
        psg.channel1.write_freq_control(0x8400);

        // Configure channel 2: 50% duty, vol=10, freq=1536
        psg.channel2.write_duty_envelope(0xA080);
        psg.channel2.write_freq_control(0x8600);

        let all_enabled = [true, true, true, true];

        // Tick and collect mixed samples (raw unsigned sum)
        let samples: Vec<i16> = (0..256)
            .map(|_| {
                psg.tick(512);
                psg.sample(all_enabled)
            })
            .collect();

        let max_sample = *samples.iter().max().unwrap();
        assert!(
            max_sample > 0,
            "mixed output should have nonzero samples"
        );
        // Max possible: ch1(15) + ch2(10) = 25
        assert!(
            max_sample <= 25,
            "mixed output should not exceed sum of channel volumes, got {}",
            max_sample
        );
    }

    #[test]
    fn psg_status_bits_reflect_active_channels() {
        let mut psg = Psg::default();
        assert_eq!(psg.status_bits(), 0);

        psg.channel1.write_duty_envelope(0xF080);
        psg.channel1.write_freq_control(0x8400);
        assert_eq!(psg.status_bits() & 1, 1, "ch1 should be active");

        psg.channel3.write_bank_control(0x0080);
        psg.channel3.write_length_volume(0x2000);
        psg.channel3.write_freq_control(0x8700);
        assert_eq!(psg.status_bits() & 4, 4, "ch3 should be active");
    }

    // ─── Waveform frequency verification ────────────────────────

    #[test]
    fn ch1_correct_frequency_period() {
        // freq_val=1920 → period per duty step = (2048-1920)*16 = 2048 cpu cycles
        // Full 8-step cycle = 2048*8 = 16384 cpu cycles
        // At 512 cycles/sample: 16384/512 = 32 samples per cycle
        let mut ch = PsgChannel1::default();
        ch.write_duty_envelope(0xF080); // vol=15, 50% duty
        ch.write_freq_control(0x8000 | 1920); // freq=1920, trigger

        // Collect 320 samples = 10 full cycles
        let samples = collect_ch1_samples(&mut ch, 320);

        // Count rising edges (0→nonzero transitions) ≈ number of cycles
        let rising_edges = samples
            .windows(2)
            .filter(|w| w[0] == 0 && w[1] > 0)
            .count();

        // Expected: 10 cycles → 10 rising edges (±1 for boundary)
        assert!(
            (9..=11).contains(&rising_edges),
            "expected ~10 rising edges for 10 cycles, got {}",
            rising_edges
        );
    }
}
