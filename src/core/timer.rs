use super::interrupt::{Interrupt, IrqBitmask};
use super::sysbus::SysBus;
use super::SyncedIoDevice;

use num::FromPrimitive;

#[derive(Debug, Default)]
pub struct Timer {
    // registers
    pub timer_ctl: TimerCtl,
    pub timer_data: u16,

    timer_id: usize,
    reg: u32,
    target: u32,
    fired: bool,
    pub initial_data: u16,

    pub cycles: usize,
}

pub enum TimerAction {
    Overflow(usize),
    Increment,
}

impl Timer {
    pub fn new(timer_id: usize) -> Timer {
        if timer_id > 3 {
            panic!("invalid timer id {}", timer_id);
        }
        Timer {
            timer_id: timer_id,
            ..Timer::default()
        }
    }

    fn get_irq(&self) -> Interrupt {
        Interrupt::from_usize(self.timer_id + 8).unwrap()
    }

    fn frequency(&self) -> usize {
        match self.timer_ctl.prescalar() {
            0 => 1,
            1 => 64,
            2 => 256,
            3 => 1024,
            _ => unreachable!(),
        }
    }

    pub fn add_cycles(&mut self, cycles: usize, irqs: &mut IrqBitmask) -> TimerAction {
        let mut num_overflows = 0;
        self.cycles += cycles;

        let frequency = self.frequency();
        while self.cycles >= frequency {
            self.cycles -= frequency;
            self.timer_data = self.timer_data.wrapping_add(1);
            if self.timer_data == 0 {
                if self.timer_ctl.irq_enabled() {
                    irqs.add_irq(self.get_irq());
                }
                self.timer_data = self.initial_data;
                num_overflows += 1;
            }
        }
        if num_overflows > 0 {
            return TimerAction::Overflow(num_overflows);
        } else {
            return TimerAction::Increment;
        }
    }
}

#[derive(Debug)]
pub struct Timers([Timer; 4]);

impl std::ops::Index<usize> for Timers {
    type Output = Timer;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl std::ops::IndexMut<usize> for Timers {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

impl Timers {
    pub fn new() -> Timers {
        Timers([Timer::new(0), Timer::new(1), Timer::new(2), Timer::new(3)])
    }
}

impl SyncedIoDevice for Timers {
    fn step(&mut self, cycles: usize, _sb: &mut SysBus, irqs: &mut IrqBitmask) {
        for i in 0..4 {
            if self[i].timer_ctl.enabled() && !self[i].timer_ctl.cascade() {
                match self[i].add_cycles(cycles, irqs) {
                    TimerAction::Overflow(num_overflows) => match i {
                        3 => {}
                        _ => {
                            let next_i = i + 1;
                            if self[next_i].timer_ctl.cascade() {
                                self[next_i].add_cycles(num_overflows, irqs);
                            }
                        }
                    },
                    TimerAction::Increment => {}
                }
            }
        }
    }
}

bitfield! {
    #[derive(Default)]
    pub struct TimerCtl(u16);
    impl Debug;
    u16;
    prescalar, _ : 1, 0;
    cascade, _ : 2;
    irq_enabled, _ : 6;
    enabled, set_enabled : 7;
}
