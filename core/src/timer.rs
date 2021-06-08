use super::dma::DmaController;
use super::interrupt::{self, Interrupt, InterruptConnect, SharedInterruptFlags};
use super::iodev::consts::*;
use super::sched::{EventType, Scheduler, SchedulerConnect, SharedScheduler};
use super::sound::SoundController;

use num::FromPrimitive;
use serde::{Deserialize, Serialize};

const SHIFT_LUT: [usize; 4] = [0, 6, 8, 10];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Timer {
    // registers
    pub ctl: TimerCtl,
    pub data: u16,
    pub initial_data: u16,

    start_time: usize,
    is_scheduled: bool,

    irq: Interrupt,
    interrupt_flags: SharedInterruptFlags,
    timer_id: usize,
    prescalar_shift: usize,
}

impl Timer {
    pub fn new(timer_id: usize, interrupt_flags: SharedInterruptFlags) -> Timer {
        if timer_id > 3 {
            panic!("invalid timer id {}", timer_id);
        }
        Timer {
            timer_id: timer_id,
            irq: Interrupt::from_usize(timer_id + 3).unwrap(),
            interrupt_flags,
            data: 0,
            ctl: TimerCtl(0),
            initial_data: 0,
            prescalar_shift: 0,
            start_time: 0,
            is_scheduled: false,
        }
    }

    #[inline]
    fn ticks_to_overflow(&self) -> u32 {
        0x1_0000 - (self.data as u32)
    }

    #[inline]
    fn sync_timer_data(&mut self, timestamp: usize) {
        let ticks_passed = (timestamp - self.start_time) >> self.prescalar_shift;
        self.data += ticks_passed as u16;
    }

    #[inline]
    fn overflow(&mut self) {
        // reload counter
        self.data = self.initial_data;
        if self.ctl.irq_enabled() {
            interrupt::signal_irq(&self.interrupt_flags, self.irq);
        }
    }

    /// increments the timer with an amount of ticks
    /// returns the number of times it overflowed
    fn update(&mut self, ticks: usize) -> usize {
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
                interrupt::signal_irq(&self.interrupt_flags, self.irq);
            }
        }

        self.data += ticks as u16;

        num_overflows as usize
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Timers {
    #[serde(skip)]
    #[serde(default = "Scheduler::new_shared")]
    scheduler: SharedScheduler,
    timers: [Timer; 4],
    running_timers: u8,

    #[cfg(feature = "debugger")]
    pub trace: bool,
}

impl InterruptConnect for Timers {
    fn connect_irq(&mut self, interrupt_flags: SharedInterruptFlags) {
        for timer in &mut self.timers {
            timer.interrupt_flags = interrupt_flags.clone();
        }
    }
}

impl SchedulerConnect for Timers {
    fn connect_scheduler(&mut self, scheduler: SharedScheduler) {
        self.scheduler = scheduler;
    }
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
    pub fn new(interrupt_flags: SharedInterruptFlags, scheduler: SharedScheduler) -> Timers {
        Timers {
            scheduler,
            timers: [
                Timer::new(0, interrupt_flags.clone()),
                Timer::new(1, interrupt_flags.clone()),
                Timer::new(2, interrupt_flags.clone()),
                Timer::new(3, interrupt_flags.clone()),
            ],
            running_timers: 0,

            #[cfg(feature = "debugger")]
            trace: false,
        }
    }

    fn add_timer_event(&mut self, id: usize) {
        let timer = &mut self.timers[id];
        timer.is_scheduled = true;
        timer.start_time = self.scheduler.timestamp();
        let cycles = (timer.ticks_to_overflow() as usize) << timer.prescalar_shift;
        self.scheduler.push(EventType::TimerOverflow(id), cycles);
    }

    fn cancel_timer_event(&mut self, id: usize) {
        self.scheduler.cancel(EventType::TimerOverflow(id));
        self[id].is_scheduled = false;
    }

    fn handle_timer_overflow(
        &mut self,
        id: usize,
        apu: &mut SoundController,
        dmac: &mut DmaController,
    ) {
        self[id].overflow();
        if id != 3 {
            let next_timer_id = id + 1;
            let next_timer = &mut self.timers[next_timer_id];
            if next_timer.ctl.cascade() {
                if next_timer.update(1) > 0 {
                    drop(next_timer);
                    self.handle_timer_overflow(next_timer_id, apu, dmac);
                }
            }
        }
        if id == 0 || id == 1 {
            apu.handle_timer_overflow(dmac, id, 1);
        }
    }

    pub fn handle_overflow_event(
        &mut self,
        id: usize,
        extra_cycles: usize,
        apu: &mut SoundController,
        dmac: &mut DmaController,
    ) {
        self.handle_timer_overflow(id, apu, dmac);

        // TODO: re-use add_timer_event function
        let timer = &mut self.timers[id];
        timer.is_scheduled = true;
        timer.start_time = self.scheduler.timestamp() - extra_cycles;
        let cycles = (timer.ticks_to_overflow() as usize) << timer.prescalar_shift;
        self.scheduler
            .push(EventType::TimerOverflow(id), cycles - extra_cycles);
    }

    pub fn write_timer_ctl(&mut self, id: usize, value: u16) {
        let timer = &mut self.timers[id];
        let new_ctl = TimerCtl(value);
        #[cfg(feature = "debugger")]
        let old_enabled = timer.ctl.enabled();
        let new_enabled = new_ctl.enabled();
        let cascade = new_ctl.cascade();
        timer.prescalar_shift = SHIFT_LUT[new_ctl.prescalar() as usize];
        timer.ctl = new_ctl;
        if new_enabled && !cascade {
            self.running_timers |= 1 << id;
            self.cancel_timer_event(id);
            self.add_timer_event(id);
        } else {
            self.running_timers &= !(1 << id);
            self.cancel_timer_event(id);
        }

        #[cfg(feature = "debugger")]
        {
            if self.trace && old_enabled != new_enabled {
                trace!(
                    "TMR{} {}",
                    id,
                    if new_enabled { "enabled" } else { "disabled" }
                );
            }
        }
    }

    #[inline]
    fn read_timer_data(&mut self, id: usize) -> u16 {
        let timer = &mut self.timers[id];
        if timer.is_scheduled {
            // this timer is controlled by the scheduler so we need to manually calculate
            // the current value of the counter
            timer.sync_timer_data(self.scheduler.timestamp());
        }

        timer.data
    }

    pub fn handle_read(&mut self, io_addr: u32) -> u16 {
        match io_addr {
            REG_TM0CNT_H => self.timers[0].ctl.0,
            REG_TM1CNT_H => self.timers[1].ctl.0,
            REG_TM2CNT_H => self.timers[2].ctl.0,
            REG_TM3CNT_H => self.timers[3].ctl.0,
            REG_TM0CNT_L => self.read_timer_data(0),
            REG_TM1CNT_L => self.read_timer_data(1),
            REG_TM2CNT_L => self.read_timer_data(2),
            REG_TM3CNT_L => self.read_timer_data(3),
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
