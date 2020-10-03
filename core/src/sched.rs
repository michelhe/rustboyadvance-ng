use std::cell::UnsafeCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

const NUM_EVENTS: usize = 32;

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
pub enum GpuEvent {
    HDraw,
    HBlank,
    VBlankHDraw,
    VBlankHBlank,
}

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
pub enum ApuEvent {
    Psg1Generate,
    Psg2Generate,
    Psg3Generate,
    Psg4Generate,
    Sample,
}

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
pub enum EventType {
    Gpu(GpuEvent),
    Apu(ApuEvent),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Event {
    typ: EventType,
    /// Timestamp in cycles
    time: usize,
}

impl Event {
    fn new(typ: EventType, time: usize) -> Event {
        Event { typ, time }
    }

    fn get_type(&self) -> EventType {
        self.typ
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Scheduler {
    timestamp: usize,
    events: Vec<Event>,
}

// Opt-out of runtime borrow checking by using unsafe cell
// SAFETY: We need to make sure that the scheduler event queue is not modified while iterating it.
#[repr(transparent)]
#[derive(Debug)]
pub struct SharedScheduler(Rc<UnsafeCell<Scheduler>>);

impl std::ops::Deref for SharedScheduler {
    type Target = Scheduler;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.0.get()) }
    }
}

impl std::ops::DerefMut for SharedScheduler {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.0.get()) }
    }
}

impl Clone for SharedScheduler {
    fn clone(&self) -> SharedScheduler {
        SharedScheduler(self.0.clone())
    }
}

pub trait EventHandler {
    /// Handle the scheduler event
    fn handle_event(&mut self, e: EventType, extra_cycles: usize);
}

impl Scheduler {
    pub fn new_shared() -> SharedScheduler {
        let sched = Scheduler {
            timestamp: 0,
            events: Vec::with_capacity(NUM_EVENTS),
        };
        SharedScheduler(Rc::new(UnsafeCell::new(sched)))
    }

    pub fn make_shared(self) -> SharedScheduler {
        SharedScheduler(Rc::new(UnsafeCell::new(self)))
    }

    pub fn schedule(&mut self, typ: EventType, cycles: usize) {
        let event = Event::new(typ, self.timestamp + cycles);
        let idx = self
            .events
            .binary_search_by(|e| e.time.cmp(&event.time))
            .unwrap_or_else(|x| x);
        self.events.insert(idx, event);
    }

    pub fn add_gpu_event(&mut self, e: GpuEvent, cycles: usize) {
        self.schedule(EventType::Gpu(e), cycles);
    }

    pub fn add_apu_event(&mut self, e: ApuEvent, cycles: usize) {
        self.schedule(EventType::Apu(e), cycles);
    }

    pub fn run<H: EventHandler>(&mut self, cycles: usize, handler: &mut H) {
        let run_to = self.timestamp + cycles;
        self.timestamp = run_to;

        while self.events.len() > 0 {
            if run_to >= self.events[0].time {
                let event = self.events.remove(0);
                handler.handle_event(event.get_type(), run_to - event.time);
            } else {
                return;
            }
        }
    }

    pub fn get_cycles_to_next_event(&self) -> usize {
        assert_ne!(self.events.len(), 0);
        self.events[0].time - self.timestamp
    }

    #[allow(unused)]
    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod test {

    use super::*;

    /// Some usecase example where a struct holds the scheduler
    struct Holder {
        sched: SharedScheduler,
        event_bitmask: u32,
    }

    const BIT_GPU_VBLANKHDRAW: u32 = 1 << 0;
    const BIT_APU_PSG1GENERATE: u32 = 1 << 1;
    const BIT_APU_PSG2GENERATE: u32 = 1 << 2;
    const BIT_APU_PSG3GENERATE: u32 = 1 << 3;
    const BIT_APU_PSG4GENERATE: u32 = 1 << 4;
    const BIT_APU_SAMPLE: u32 = 1 << 5;

    #[inline]
    fn get_event_bit(e: EventType) -> u32 {
        match e {
            EventType::Gpu(GpuEvent::VBlankHDraw) => BIT_GPU_VBLANKHDRAW,
            EventType::Apu(ApuEvent::Psg1Generate) => BIT_APU_PSG1GENERATE,
            EventType::Apu(ApuEvent::Psg2Generate) => BIT_APU_PSG2GENERATE,
            EventType::Apu(ApuEvent::Psg3Generate) => BIT_APU_PSG3GENERATE,
            EventType::Apu(ApuEvent::Psg4Generate) => BIT_APU_PSG4GENERATE,
            EventType::Apu(ApuEvent::Sample) => BIT_APU_SAMPLE,
            _ => unimplemented!("unsupported event for this test"),
        }
    }

    impl Holder {
        fn new() -> Holder {
            Holder {
                sched: Scheduler::new_shared(),
                event_bitmask: 0,
            }
        }

        fn is_event_done(&self, e: EventType) -> bool {
            (self.event_bitmask & get_event_bit(e)) != 0
        }
    }

    impl EventHandler for Holder {
        fn handle_event(&mut self, e: EventType, extra_cycles: usize) {
            println!("[holder] got event {:?} extra_cycles {}", e, extra_cycles);
            self.event_bitmask |= get_event_bit(e);
        }
    }

    #[test]
    fn test_scheduler() {
        let mut holder = Holder::new();

        // clone the sched so we get a reference that is not owned by the holder
        // SAFETY: since the SharedScheduler is built upon an UnsafeCell instead of RefCell, we are sacrificing runtime safety checks for performance.
        //  It is safe since the events iteration allows the EventHandler to modify the queue.

        let mut sched = holder.sched.clone();
        holder
            .sched
            .schedule(EventType::Gpu(GpuEvent::VBlankHDraw), 240);
        holder
            .sched
            .schedule(EventType::Apu(ApuEvent::Psg1Generate), 60);
        holder.sched.schedule(EventType::Apu(ApuEvent::Sample), 512);
        holder
            .sched
            .schedule(EventType::Apu(ApuEvent::Psg2Generate), 13);
        holder
            .sched
            .schedule(EventType::Apu(ApuEvent::Psg4Generate), 72);

        println!("all events");
        for e in sched.events.iter() {
            let typ = e.get_type();
            println!("{:?}", typ);
        }

        macro_rules! run_for {
            ($cycles:expr) => {
                println!("running the scheduler for {} cycles", $cycles);
                sched.run($cycles, &mut holder);
                if (!sched.is_empty()) {
                    println!(
                        "cycles for next event: {}",
                        sched.get_cycles_to_next_event()
                    );
                }
            };
        }

        run_for!(100);

        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Psg1Generate)),
            true
        );
        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Psg2Generate)),
            true
        );
        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Psg4Generate)),
            true
        );
        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Sample)),
            false
        );
        assert_eq!(
            holder.is_event_done(EventType::Gpu(GpuEvent::VBlankHDraw)),
            false
        );

        run_for!(100);

        assert_eq!(
            holder.is_event_done(EventType::Gpu(GpuEvent::VBlankHDraw)),
            false
        );
        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Sample)),
            false
        );

        run_for!(100);

        assert_eq!(
            holder.is_event_done(EventType::Gpu(GpuEvent::VBlankHDraw)),
            true
        );
        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Sample)),
            false
        );

        run_for!(211);

        assert_eq!(
            holder.is_event_done(EventType::Apu(ApuEvent::Sample)),
            false
        );

        run_for!(1);

        assert_eq!(holder.is_event_done(EventType::Apu(ApuEvent::Sample)), true);

        println!("all events (holder)");
        for e in holder.sched.events.iter() {
            let typ = e.get_type();
            println!("{:?}", typ);
        }

        println!("all events (cloned again)");
        let sched_cloned = holder.sched.clone();
        for e in sched_cloned.events.iter() {
            let typ = e.get_type();
            println!("{:?}", typ);
        }
    }
}
