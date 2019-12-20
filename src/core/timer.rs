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

pub enum TimerEvent {
    Overflow(usize, usize),
    Increment(usize),
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

    pub fn add_cycles(&mut self, cycles: usize, irqs: &mut IrqBitmask) -> TimerEvent {
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
            return TimerEvent::Overflow(self.timer_id, num_overflows);
        } else {
            return TimerEvent::Increment(self.timer_id);
        }
    }
}

#[derive(Debug)]
pub struct Timers {
    timers: [Timer; 4],
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
            trace: false,
        }
    }

    pub fn write_timer_ctl(&mut self, id: usize, value: u16) {
        let old_enabled = self[id].timer_ctl.enabled();
        self[id].timer_ctl.0 = value;
        let new_enabled = self[id].timer_ctl.enabled();
        if self.trace && old_enabled != new_enabled {
            println!(
                "TMR{} {}",
                id,
                if new_enabled { "enabled" } else { "disabled" }
            );
        }
    }

    pub fn tick(
        &mut self,
        cycles: usize,
        sb: &mut SysBus,
        irqs: &mut IrqBitmask,
    ) -> Option<TimerEvent> {
        for i in 0..4 {
            if self[i].timer_ctl.enabled() && !self[i].timer_ctl.cascade() {
                let event = self[i].add_cycles(cycles, irqs);
                match event {
                    TimerEvent::Overflow(_, num_overflows) => {
                        if self.trace {
                            println!("TMR{} overflown!", i);
                        }
                        if i != 3 {
                            let next_i = i + 1;
                            if self[next_i].timer_ctl.cascade() {
                                self[next_i].add_cycles(num_overflows, irqs);
                            }
                        }
                        if i == 0 || i == 1 {
                            sb.io.sound.handle_timer_overflow(&mut sb.io.dmac, i);
                        }
                    }
                    _ => {}
                }
                return Some(event);
            }
        }
        None
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
