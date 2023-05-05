use std::cmp::Ordering;
use std::collections::BinaryHeap;

use rustboyadvance_utils::Shared;

use serde::{Deserialize, Serialize};

const NUM_EVENTS: usize = 32;

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialOrd, PartialEq, Eq, Copy, Clone)]
pub enum GpuEvent {
    HDraw,
    HBlank,
    VBlankHDraw,
    VBlankHBlank,
}

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialOrd, PartialEq, Eq, Copy, Clone)]
pub enum ApuEvent {
    Psg1Generate,
    Psg2Generate,
    Psg3Generate,
    Psg4Generate,
    Sample,
}

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, PartialOrd, PartialEq, Eq, Copy, Clone)]
pub enum EventType {
    RunLimitReached,
    Gpu(GpuEvent),
    Apu(ApuEvent),
    DmaActivateChannel(usize),
    TimerOverflow(usize),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq)]
pub struct Event {
    typ: EventType,
    /// Timestamp in cycles
    time: usize,
}

impl Event {
    pub fn new(typ: EventType, time: usize) -> Event {
        Event { typ, time }
    }

    #[inline]
    fn get_type(&self) -> EventType {
        self.typ
    }
}

/// Future event is an event to be scheduled in x cycles from now
pub type FutureEvent = (EventType, usize);

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time).reverse()
    }
}

/// Implement custom reverse ordering
impl PartialOrd for Event {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.time.partial_cmp(&self.time)
    }

    #[inline]
    fn lt(&self, other: &Self) -> bool {
        other.time < self.time
    }
    #[inline]
    fn le(&self, other: &Self) -> bool {
        other.time <= self.time
    }
    #[inline]
    fn gt(&self, other: &Self) -> bool {
        other.time > self.time
    }
    #[inline]
    fn ge(&self, other: &Self) -> bool {
        other.time >= self.time
    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

/// Event scheduelr for cycle aware components
/// The scheduler should be "shared" to all event generating components.
/// Each event generator software component can call Scheduler::schedule to generate an event later in the emulation.
/// The scheduler should be updated for each increment in CPU cycles,
///
/// The main emulation loop can then call Scheduler::process_pending to handle the events.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Scheduler {
    timestamp: usize,
    events: BinaryHeap<Event>,
}

pub type SharedScheduler = Shared<Scheduler>;

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler {
            timestamp: 0,
            events: BinaryHeap::with_capacity(NUM_EVENTS),
        }
    }

    pub fn new_shared() -> SharedScheduler {
        Scheduler::new().make_shared()
    }

    pub fn make_shared(self) -> SharedScheduler {
        SharedScheduler::new(self)
    }

    #[inline]
    #[allow(unused)]
    pub fn num_pending_events(&self) -> usize {
        self.events.len()
    }

    #[inline]
    #[allow(unused)]
    pub fn peek_next(&self) -> Option<EventType> {
        self.events.peek().map(|e| e.typ)
    }

    /// Schedule an event to be executed in `when` cycles from now
    pub fn schedule(&mut self, event: FutureEvent) {
        let (typ, when) = event;
        let event = Event::new(typ, self.timestamp + when);
        self.events.push(event);
    }

    /// Schedule an event to be executed at an exact timestamp, can be used to schedule "past" events.
    pub fn schedule_at(&mut self, event_typ: EventType, timestamp: usize) {
        self.events.push(Event::new(event_typ, timestamp));
    }

    /// Cancel all events with type `typ`
    pub fn cancel_pending(&mut self, typ: EventType) {
        self.events.retain(|e| e.typ != typ);
    }

    /// Updates the scheduler timestamp
    #[inline]
    pub fn update(&mut self, cycles: usize) {
        self.timestamp += cycles;
    }

    pub fn pop_pending_event(&mut self) -> Option<(EventType, usize)> {
        let Some(event) = self.events.peek() else {
            return None
        };
        if self.timestamp >= event.time {
            return None
        }
        // SAFETY: events.peek() above guarantees that event exists
        let event = unsafe { self.events.pop().unwrap_unchecked() };
        Some((event.get_type(), event.time))
    }

    #[inline]
    pub fn fast_forward_to_next(&mut self) {
        self.timestamp += self.get_cycles_to_next_event();
    }

    #[inline]
    pub fn get_cycles_to_next_event(&self) -> usize {
        self.events
            .peek()
            .map(|event| event.time - self.timestamp)
            .unwrap_or(0)
    }

    #[inline]
    /// Safety - Onyl safe to call when we know the event queue is not empty
    pub unsafe fn timestamp_of_next_event_unchecked(&self) -> usize {
        self.events
            .peek()
            .unwrap_unchecked()
            .time
    }

    #[inline]
    pub fn timestamp(&self) -> usize {
        self.timestamp
    }

    #[allow(unused)]
    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn measure_cycles<F: FnMut()>(&mut self, mut f: F) -> usize {
        let start = self.timestamp;
        f();
        self.timestamp - start
    }
}

pub trait SchedulerConnect {
    fn connect_scheduler(&mut self, scheduler: SharedScheduler);
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
        fn handle_event(&mut self, e: EventType, extra_cycles: usize) {
            println!("[holder] got event {:?} extra_cycles {}", e, extra_cycles);
            self.event_bitmask |= get_event_bit(e);
        }
    }

    #[test]
    fn test_scheduler_ordering() {
        let mut holder = Holder::new();
        let mut sched = holder.sched.clone();
        holder
            .sched
            .schedule((EventType::Gpu(GpuEvent::VBlankHDraw), 240));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg1Generate), 60));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Sample), 512));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg2Generate), 13));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg4Generate), 72));

        assert_eq!(
            sched.events.pop(),
            Some(Event::new(EventType::Apu(ApuEvent::Psg2Generate), 13))
        );
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
            .schedule((EventType::Gpu(GpuEvent::VBlankHDraw), 240));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg1Generate), 60));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Sample), 512));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg2Generate), 13));
        holder
            .sched
            .schedule((EventType::Apu(ApuEvent::Psg4Generate), 72));

        println!("all events");
        for e in sched.events.iter() {
            let typ = e.get_type();
            println!("{:?}", typ);
        }

        macro_rules! run_for {
            ($cycles:expr) => {
                println!("running the scheduler for {} cycles", $cycles);
                sched.update($cycles);
                while let Some((event, cycles_late)) = sched.pop_pending_event() {
                    holder.handle_event(event, cycles_late);
                }
                if (!sched.is_empty()) {
                    println!(
                        "cycles for next event: {}",
                        sched.get_cycles_to_next_event()
                    );
                }
            };
        }

        run_for!(100);

        println!("{:?}", *sched);
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
