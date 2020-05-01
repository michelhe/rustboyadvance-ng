use super::interrupt::{Interrupt, IrqBitmask};
use super::iodev::consts::*;
use super::sysbus::SysBus;

use num::FromPrimitive;
use serde::{Deserialize, Serialize};

const SHIFT_LUT: [usize; 4] = [0, 6, 8, 10];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Timer {
    // registers
    pub ctl: TimerCtl,
    pub data: u16,
    pub initial_data: u16,

    irq: Interrupt,
    timer_id: usize,
    cycles: usize,
    prescalar_shift: usize,
}

impl Timer {
    pub fn new(timer_id: usize) -> Timer {
        if timer_id > 3 {
            panic!("invalid timer id {}", timer_id);
        }
        Timer {
            timer_id: timer_id,
            irq: Interrupt::from_usize(timer_id + 3).unwrap(),
            data: 0,
            ctl: TimerCtl(0),
            initial_data: 0,
            cycles: 0,
            prescalar_shift: 0,
        }
    }

    #[inline]
    fn ticks_to_overflow(&self) -> u32 {
        0x1_0000 - (self.data as u32)
    }

    /// increments the timer with an amount of ticks
    /// returns the number of times it overflowed
    fn update(&mut self, ticks: usize, irqs: &mut IrqBitmask) -> usize {
        let mut ticks = ticks as u32;
        let mut num_overflows = 0;

        let ticks_remaining = self.ticks_to_overflow();

        if ticks >= ticks_remaining {
            num_overflows += 1;
            ticks -= ticks_remaining;
            self.data = self.initial_data;

            let ticks_remaining = self.ticks_to_overflow();
            num_overflows += ticks / ticks_remaining;
            ticks = ticks % ticks_remaining;

            if self.ctl.irq_enabled() {
                irqs.add_irq(self.irq);
            }
        }

        self.data += ticks as u16;

        num_overflows as usize
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Timers {
    timers: [Timer; 4],
    running_timers: u8,
    pub trace: bool,
}

impl std::ops::Index<usize> for Timers {
    type Output = Timer;
    fn index(&self, index: usize) -> &Self::Output {
        &self.timers[index]
    }
}

impl std::ops::IndexMut<usize> for Timers {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.timers[index]
    }
}

impl Timers {
    pub fn new() -> Timers {
        Timers {
            timers: [Timer::new(0), Timer::new(1), Timer::new(2), Timer::new(3)],
            running_timers: 0,
            trace: false,
        }
    }

    pub fn write_timer_ctl(&mut self, id: usize, value: u16) {
        let new_ctl = TimerCtl(value);
        let old_enabled = self[id].ctl.enabled();
        let new_enabled = new_ctl.enabled();
        let cascade = new_ctl.cascade();
        self[id].cycles = 0;
        self[id].prescalar_shift = SHIFT_LUT[new_ctl.prescalar() as usize];
        self[id].ctl = new_ctl;
        if new_enabled && !cascade {
            self.running_timers |= 1 << id;
        } else {
            self.running_timers &= !(1 << id);
        }
        if old_enabled != new_enabled {
            trace!(
                "TMR{} {}",
                id,
                if new_enabled { "enabled" } else { "disabled" }
            );
        }
    }

    pub fn handle_read(&self, io_addr: u32) -> u16 {
        match io_addr {
            REG_TM0CNT_L => self.timers[0].data,
            REG_TM0CNT_H => self.timers[0].ctl.0,
            REG_TM1CNT_L => self.timers[1].data,
            REG_TM1CNT_H => self.timers[1].ctl.0,
            REG_TM2CNT_L => self.timers[2].data,
            REG_TM2CNT_H => self.timers[2].ctl.0,
            REG_TM3CNT_L => self.timers[3].data,
            REG_TM3CNT_H => self.timers[3].ctl.0,
            _ => unreachable!(),
        }
    }

    pub fn handle_write(&mut self, io_addr: u32, value: u16) {
        match io_addr {
            REG_TM0CNT_L => {
                self.timers[0].data = value;
                self.timers[0].initial_data = value;
            }
            REG_TM0CNT_H => self.write_timer_ctl(0, value),

            REG_TM1CNT_L => {
                self.timers[1].data = value;
                self.timers[1].initial_data = value;
            }
            REG_TM1CNT_H => self.write_timer_ctl(1, value),

            REG_TM2CNT_L => {
                self.timers[2].data = value;
                self.timers[2].initial_data = value;
            }
            REG_TM2CNT_H => self.write_timer_ctl(2, value),

            REG_TM3CNT_L => {
                self.timers[3].data = value;
                self.timers[3].initial_data = value;
            }
            REG_TM3CNT_H => self.write_timer_ctl(3, value),
            _ => unreachable!(),
        }
    }

    pub fn update(&mut self, cycles: usize, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        for id in 0..4 {
            if self.running_timers & (1 << id) == 0 {
                continue;
            }

            if !self.timers[id].ctl.cascade() {
                let timer = &mut self.timers[id];

                let cycles = timer.cycles + cycles;
                let inc = cycles >> timer.prescalar_shift;
                let num_overflows = timer.update(inc, irqs);
                timer.cycles = cycles & ((1 << timer.prescalar_shift) - 1);

                if num_overflows > 0 {
                    if id != 3 {
                        let next_timer = &mut self.timers[id + 1];
                        if next_timer.ctl.cascade() {
                            next_timer.update(num_overflows, irqs);
                        }
                    }
                    if id == 0 || id == 1 {
                        sb.io
                            .sound
                            .handle_timer_overflow(&mut sb.io.dmac, id, num_overflows);
                    }
                }
            }
        }
    }
}

bitfield! {
    #[derive(Serialize, Deserialize, Clone, Default)]
    pub struct TimerCtl(u16);
    impl Debug;
    u16;
    prescalar, _ : 1, 0;
    cascade, _ : 2;
    irq_enabled, _ : 6;
    enabled, set_enabled : 7;
}
