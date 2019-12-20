use bit::BitIndex;

use super::dma::DmaController;
use super::iodev::consts::*;
use super::iodev::io_reg_string;

mod fifo;
use fifo::SoundFifo;

const DMG_RATIOS: [f32; 4] = [0.25, 0.5, 1.0, 0.0];
const DMA_RATIOS: [f32; 2] = [0.5, 1.0];
const DMA_TIMERS: [usize; 2] = [0, 1];
const DUTY_RATIOS: [f32; 4] = [0.125, 0.25, 0.5, 0.75];

#[derive(Debug)]
struct NoiseChannel {}

#[derive(Debug)]
struct DmaSound {
    volume: f32,
    enable_right: bool,
    enable_left: bool,
    timer_select: usize,
    fifo: SoundFifo<i8>,
}

impl Default for DmaSound {
    fn default() -> DmaSound {
        DmaSound {
            volume: DMA_RATIOS[0],
            enable_right: false,
            enable_left: false,
            timer_select: 0,
            fifo: SoundFifo::new(),
        }
    }
}

#[derive(Debug)]
pub struct SoundController {
    sample_rate_to_cpu_freq: usize, // how many "cycles" are a sample?
    last_sample_cycles: usize,      // cycles count when we last provided a new sample.

    mse: bool,

    left_volume: usize,
    left_sqr1: bool,
    left_sqr2: bool,
    left_wave: bool,
    left_noise: bool,

    right_volume: usize,
    right_sqr1: bool,
    right_sqr2: bool,
    right_wave: bool,
    right_noise: bool,

    dmg_volume_ratio: f32,

    sqr1_rate: usize,
    sqr1_timed: bool,
    sqr1_length: f32,
    sqr1_duty: f32,
    sqr1_step_time: usize,
    sqr1_step_increase: bool,
    sqr1_initial_vol: usize,
    sqr1_cur_vol: usize,

    sound_a: DmaSound,
    sound_b: DmaSound,
}

impl SoundController {
    pub fn new() -> SoundController {
        SoundController {
            sample_rate_to_cpu_freq: 12345,
            last_sample_cycles: 0,
            mse: false,
            left_volume: 0,
            left_sqr1: false,
            left_sqr2: false,
            left_wave: false,
            left_noise: false,
            right_volume: 0,
            right_sqr1: false,
            right_sqr2: false,
            right_wave: false,
            right_noise: false,
            dmg_volume_ratio: 0.0,
            sqr1_rate: 0,
            sqr1_timed: false,
            sqr1_length: 0.0,
            sqr1_duty: DUTY_RATIOS[0],
            sqr1_step_time: 0,
            sqr1_step_increase: false,
            sqr1_initial_vol: 0,
            sqr1_cur_vol: 0,
            sound_a: Default::default(),
            sound_b: Default::default(),
        }
    }

    pub fn handle_read(&self, io_addr: u32) -> u16 {
        let value = match io_addr {
            REG_SOUNDCNT_X => cbit(7, self.mse),
            REG_SOUNDCNT_L => {
                self.left_volume as u16
                    | (self.right_volume as u16) << 4
                    | cbit(8, self.left_sqr1)
                    | cbit(9, self.left_sqr2)
                    | cbit(10, self.left_wave)
                    | cbit(11, self.left_noise)
                    | cbit(12, self.right_sqr1)
                    | cbit(13, self.right_sqr2)
                    | cbit(14, self.right_wave)
                    | cbit(15, self.right_noise)
            }

            REG_SOUNDCNT_H => {
                DMG_RATIOS
                    .iter()
                    .position(|&f| f == self.dmg_volume_ratio)
                    .expect("bad dmg_volume_ratio!") as u16
                    | DMA_RATIOS
                        .iter()
                        .position(|&f| f == self.sound_a.volume)
                        .unwrap() as u16
                    | DMA_RATIOS
                        .iter()
                        .position(|&f| f == self.sound_b.volume)
                        .unwrap() as u16
                    | cbit(8, self.sound_a.enable_right)
                    | cbit(9, self.sound_a.enable_left)
                    | cbit(10, self.sound_a.timer_select != 0)
                    | cbit(12, self.sound_b.enable_right)
                    | cbit(13, self.sound_b.enable_left)
                    | cbit(14, self.sound_b.timer_select != 0)
            }

            _ => {
                println!(
                    "Unimplemented read from {:x} {}",
                    io_addr,
                    io_reg_string(io_addr)
                );
                0
            }
        };
        println!(
            "Read {} ({:08x}) = {:04x}",
            io_reg_string(io_addr),
            io_addr,
            value
        );
        value
    }

    pub fn handle_write(&mut self, io_addr: u32, value: u16) {
        if io_addr == REG_SOUNDCNT_X {
            if value & bit(7) != 0 {
                if !self.mse {
                    println!("MSE enabled!");
                    self.mse = true;
                }
            } else {
                if self.mse {
                    println!("MSE disabled!");
                    self.mse = false;
                }
            }

            // other fields of this register are read-only anyway, ignore them.
            return;
        }

        if !self.mse {
            println!("MSE disabled, refusing to write");
            return;
        }

        match io_addr {
            REG_SOUNDCNT_L => {
                self.left_volume = value.bit_range(0..2) as usize;
                self.right_volume = value.bit_range(4..6) as usize;
                self.left_sqr1 = value.bit(8);
                self.left_sqr2 = value.bit(9);
                self.left_wave = value.bit(10);
                self.left_noise = value.bit(11);
                self.right_sqr1 = value.bit(12);
                self.right_sqr2 = value.bit(13);
                self.right_wave = value.bit(14);
                self.right_noise = value.bit(15);
            }

            REG_SOUNDCNT_H => {
                self.dmg_volume_ratio = DMG_RATIOS[value.bit_range(0..1) as usize];
                self.sound_a.volume = DMA_RATIOS[value.bit(2) as usize];
                self.sound_b.volume = DMA_RATIOS[value.bit(3) as usize];
                self.sound_a.enable_right = value.bit(8);
                self.sound_a.enable_left = value.bit(9);
                self.sound_a.timer_select = DMA_TIMERS[value.bit(10) as usize];
                self.sound_b.enable_right = value.bit(12);
                self.sound_b.enable_left = value.bit(13);
                self.sound_b.timer_select = DMA_TIMERS[value.bit(14) as usize];

                if value.bit(11) {
                    self.sound_a.fifo.reset();
                }
                if value.bit(15) {
                    self.sound_b.fifo.reset();
                }
            }

            REG_SOUND1CNT_H => {
                self.sqr1_length = (64 - value.bit_range(0..5) as usize) as f32 / 256.0;
                self.sqr1_duty = DUTY_RATIOS[value.bit_range(6..7) as usize];
                self.sqr1_step_time = value.bit_range(8..10) as usize;
                self.sqr1_step_increase = value.bit(11);
                self.sqr1_initial_vol = value.bit_range(12..15) as usize;
            }

            REG_SOUND1CNT_X => {
                self.sqr1_rate = value.bit_range(0..10) as usize;
                self.sqr1_timed = value.bit(14);
                if value.bit(15) {
                    self.sqr1_cur_vol = self.sqr1_initial_vol;
                }
            }

            REG_FIFO_A => {
                self.sound_a.fifo.write((value & 0xff00 >> 8) as i8);
                self.sound_a.fifo.write((value & 0xff) as i8);
            }

            REG_FIFO_B => {
                self.sound_b.fifo.write((value & 0xff00 >> 8) as i8);
                self.sound_b.fifo.write((value & 0xff) as i8);
            }

            _ => {
                println!(
                    "Unimplemented write to {:x} {}",
                    io_addr,
                    io_reg_string(io_addr)
                );
            }
        }
    }

    pub fn handle_timer_overflow(&mut self, dmac: &mut DmaController, timer_id: usize) {
        if !self.mse {
            return;
        }
        // TODO - play sound ?

        if timer_id == self.sound_a.timer_select {
            dmac.notify_sound_fifo(REG_FIFO_A);
        }
        if timer_id == self.sound_b.timer_select {
            dmac.notify_sound_fifo(REG_FIFO_B);
        }
    }

    pub fn update(&mut self, cycles: usize) {
        while cycles - self.last_sample_cycles >= self.sample_rate_to_cpu_freq {
            self.last_sample_cycles += self.sample_rate_to_cpu_freq;
        }
    }
}

// TODO move
fn cbit(idx: u8, value: bool) -> u16 {
    if value {
        1 << idx
    } else {
        0
    }
}

// TODO mvoe
fn bit(idx: u8) -> u16 {
    1 << idx
}

fn rate_to_freq(rate: u16) -> usize {
    assert!(rate < 2048);

    (2 << 17) as usize / (2048 - rate) as usize
}
