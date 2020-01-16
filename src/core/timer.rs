use super::interrupt::{Interrupt, IrqBitmask};
use super::iodev::consts::*;
use super::sysbus::SysBus;

use num::FromPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Timer {
    // registers
    pub ctl: TimerCtl,
    pub data: u16,

    irq: Interrupt,

    timer_id: usize,
    pub initial_data: u16,

    pub cycles: usize,
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
        }
    }

    fn frequency(&self) -> usize {
        match self.ctl.prescalar() {
            0 => 1,
            1 => 64,
            2 => 256,
            3 => 1024,
            _ => unreachable!(),
        }
    }

    /// updates the timer with 'cycles' amount of cycles, returns the number of times it overflowed
    fn update(&mut self, cycles: usize, irqs: &mut IrqBitmask) -> usize {
        self.cycles += cycles;
        let mut num_overflows = 0;
        let freq = self.frequency();
        while self.cycles >= freq {
            self.cycles -= freq;
            self.data = self.data.wrapping_add(1);
            if self.data == 0 {
                if self.ctl.irq_enabled() {
                    irqs.add_irq(self.irq);
                }
                self.data = self.initial_data;
                num_overflows += 1;
            }
        }

        num_overflows
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
        let old_enabled = self[id].ctl.enabled();
        self[id].ctl.0 = value;
        let new_enabled = self[id].ctl.enabled();
        let cascade = self.timers[id].ctl.cascade();
        if new_enabled && !cascade {
            self.running_timers |= 1 << id;
        } else {
            self.running_timers &= !(1 << id);
        }
        if self.trace && old_enabled != new_enabled {
            println!(
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
                let num_overflows = timer.update(cycles, irqs);
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
