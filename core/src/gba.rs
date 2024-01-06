/// Struct containing everything
use std::cell::Cell;
use std::rc::Rc;

use bincode;
use serde::{Deserialize, Serialize};

use crate::gdb_support::{gdb_thread::start_gdb_server_thread, DebuggerRequestHandler};

use super::cartridge::Cartridge;
use super::dma::DmaController;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sched::{EventType, Scheduler, SchedulerConnect, SharedScheduler};
use super::sound::SoundController;
use super::sysbus::SysBus;
use super::timer::Timers;

use super::sound::interface::DynAudioInterface;

use arm7tdmi::{self, Arm7tdmiCore};
use rustboyadvance_utils::Shared;

pub struct GameBoyAdvance {
    pub cpu: Box<Arm7tdmiCore<SysBus>>,
    pub(crate) sysbus: Shared<SysBus>,
    pub(crate) io_devs: Shared<IoDevices>,
    pub(crate) scheduler: SharedScheduler,
    interrupt_flags: SharedInterruptFlags,
    audio_interface: DynAudioInterface,
    pub(crate) debugger: Option<DebuggerRequestHandler>,
}

#[derive(Serialize, Deserialize)]
struct SaveState {
    scheduler: Scheduler,
    io_devs: IoDevices,
    cartridge: Cartridge,
    ewram: Box<[u8]>,
    iwram: Box<[u8]>,
    interrupt_flags: u16,
    cpu_state: arm7tdmi::SavedCpuState,
}

#[derive(Debug, PartialEq)]
enum BusMaster {
    Dma,
    Cpu,
}

/// Checks if the bios provided is the real one
fn check_real_bios(bios: &[u8]) -> bool {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.input(bios);
    let digest = hasher.result();

    let expected_hash = hex!("fd2547724b505f487e6dcb29ec2ecff3af35a841a77ab2e85fd87350abd36570");

    digest.as_slice() == &expected_hash[..]
}

impl GameBoyAdvance {
    pub fn new(
        bios_rom: Box<[u8]>,
        gamepak: Cartridge,
        audio_interface: DynAudioInterface,
    ) -> GameBoyAdvance {
        // Warn the user if the bios is not the real one
        match check_real_bios(&bios_rom) {
            true => info!("Verified bios rom"),
            false => warn!("This is not the real bios rom, some games may not be compatible"),
        };

        let interrupt_flags = Rc::new(Cell::new(IrqBitmask(0)));
        let mut scheduler = Scheduler::new_shared();

        let intc = InterruptController::new(interrupt_flags.clone());
        let gpu = Box::new(Gpu::new(&mut scheduler, interrupt_flags.clone()));
        let dmac = DmaController::new(interrupt_flags.clone());
        let timers = Timers::new(interrupt_flags.clone());
        let sound_controller = Box::new(SoundController::new(
            &mut scheduler,
            audio_interface.get_sample_rate() as f32,
        ));
        let io_devs = Shared::new(IoDevices::new(
            intc,
            gpu,
            dmac,
            timers,
            sound_controller,
            scheduler.clone(),
        ));
        let sysbus = Shared::new(SysBus::new(
            scheduler.clone(),
            io_devs.clone(),
            bios_rom,
            gamepak,
        ));

        let cpu = Box::new(Arm7tdmiCore::new(sysbus.clone()));

        let mut gba = GameBoyAdvance {
            cpu,
            sysbus,
            io_devs,
            audio_interface,
            scheduler,
            interrupt_flags,
            debugger: None,
        };

        gba.sysbus.init(gba.cpu.weak_ptr());

        gba
    }

    pub fn from_saved_state(
        savestate: &[u8],
        bios: Box<[u8]>,
        rom: Box<[u8]>,
        audio_interface: DynAudioInterface,
    ) -> bincode::Result<GameBoyAdvance> {
        let decoded: Box<SaveState> = bincode::deserialize_from(savestate)?;

        let interrupts = Rc::new(Cell::new(IrqBitmask(decoded.interrupt_flags)));
        let scheduler = decoded.scheduler.make_shared();
        let mut io_devs = Shared::new(decoded.io_devs);
        let mut cartridge = decoded.cartridge;
        cartridge.set_rom_bytes(rom);
        io_devs.connect_irq(interrupts.clone());
        io_devs.connect_scheduler(scheduler.clone());
        let mut sysbus = Shared::new(SysBus::new_with_memories(
            scheduler.clone(),
            io_devs.clone(),
            cartridge,
            bios,
            decoded.ewram,
            decoded.iwram,
        ));
        let mut arm7tdmi = Box::new(Arm7tdmiCore::from_saved_state(
            sysbus.clone(),
            decoded.cpu_state,
        ));

        sysbus.init(arm7tdmi.weak_ptr());

        Ok(GameBoyAdvance {
            cpu: arm7tdmi,
            sysbus,
            io_devs,
            interrupt_flags: interrupts,
            audio_interface,
            scheduler,
            debugger: None,
        })
    }

    pub fn save_state(&self) -> bincode::Result<Vec<u8>> {
        let s = SaveState {
            cpu_state: self.cpu.save_state(),
            io_devs: self.io_devs.clone_inner(),
            cartridge: self.sysbus.cartridge.thin_copy(),
            iwram: Box::from(self.sysbus.get_iwram()),
            ewram: Box::from(self.sysbus.get_ewram()),
            interrupt_flags: self.interrupt_flags.get().value(),
            scheduler: self.scheduler.clone_inner(),
        };

        bincode::serialize(&s)
    }

    pub fn restore_state(&mut self, bytes: &[u8]) -> bincode::Result<()> {
        let decoded: Box<SaveState> = bincode::deserialize_from(bytes)?;

        self.cpu.restore_state(decoded.cpu_state);
        self.scheduler = Scheduler::make_shared(decoded.scheduler);
        self.interrupt_flags = Rc::new(Cell::new(IrqBitmask(decoded.interrupt_flags)));
        self.io_devs = Shared::new(decoded.io_devs);
        // Restore memory state
        self.cpu.set_memory_interface(self.sysbus.clone());
        self.sysbus.set_iwram(decoded.iwram);
        self.sysbus.set_ewram(decoded.ewram);
        // Redistribute shared pointers
        self.io_devs.connect_irq(self.interrupt_flags.clone());
        self.sysbus.connect_scheduler(self.scheduler.clone());
        self.sysbus.set_io_devices(self.io_devs.clone());
        self.sysbus.cartridge.update_from(decoded.cartridge);
        self.sysbus.init(self.cpu.weak_ptr());

        Ok(())
    }

    pub fn get_game_title(&self) -> String {
        self.sysbus.cartridge.header.game_title.clone()
    }

    pub fn get_game_code(&self) -> String {
        self.sysbus.cartridge.header.game_code.clone()
    }

    #[inline]
    pub fn get_key_state(&mut self) -> &u16 {
        &self.sysbus.io.keyinput
    }

    #[inline]
    pub fn get_key_state_mut(&mut self) -> &mut u16 {
        &mut self.sysbus.io.keyinput
    }

    /// Advance the emulation for one frame worth of time
    pub fn frame(&mut self) {
        static mut OVERSHOOT: usize = 0;
        unsafe {
            OVERSHOOT = CYCLES_FULL_REFRESH.saturating_sub(self.run::<false>(CYCLES_FULL_REFRESH - OVERSHOOT));
        }
    }

    /// like frame() but stop if a breakpoint is reached
    fn frame_interruptible(&mut self) {
        static mut OVERSHOOT: usize = 0;
        unsafe {
            OVERSHOOT = CYCLES_FULL_REFRESH.saturating_sub(self.run::<true>(CYCLES_FULL_REFRESH - OVERSHOOT));
        }
    }

    pub fn start_gdbserver(&mut self, port: u16) {
        if self.is_debugger_attached() {
            warn!("debugger already attached!");
        } else {
            match start_gdb_server_thread(self, port) {
                Ok(debugger) => {
                    info!("attached to the debugger, have fun!");
                    self.debugger = Some(debugger)
                }
                Err(e) => {
                    error!("failed to start the debugger: {:?}", e);
                }
            }
        }
    }

    #[inline]
    pub fn is_debugger_attached(&self) -> bool {
        self.debugger.is_some()
    }

    /// Recv & handle messages from the debugger, and return if we are stopped or not
    pub fn debugger_run(&mut self) {
        let debugger = self.debugger.take().expect("debugger should be None here");
        self.debugger = debugger.handle_incoming_requests(self);
        self.frame_interruptible();
    }

    #[inline]
    fn dma_step(&mut self) {
        self.io_devs.dmac.perform_work(&mut self.sysbus);
    }

    #[inline]
    fn cpu_interrupt(&mut self) {
        self.cpu.irq();
        self.io_devs.haltcnt = HaltState::Running; // Clear out from low power mode
    }

    #[inline]
    fn cpu_step(&mut self) {
        if self.io_devs.intc.irq_pending() {
            self.cpu_interrupt();
        }
        self.cpu.step();
    }

    #[inline]
    fn get_bus_master(&mut self) -> Option<BusMaster> {
        match (self.io_devs.dmac.is_active(), self.io_devs.haltcnt) {
            (true, _) => Some(BusMaster::Dma),
            (false, HaltState::Running) => Some(BusMaster::Cpu),
            (false, HaltState::Halt) => None,
        }
    }

    #[inline]
    pub(crate) fn single_step(&mut self) {
        // 3 Options:
        // 1. DMA is active - thus CPU is blocked
        // 2. DMA inactive and halt state is RUN - CPU can run
        // 3. DMA inactive and halt state is HALT - CPU is blocked
        match self.get_bus_master() {
            Some(BusMaster::Dma) => self.dma_step(),
            Some(BusMaster::Cpu) => self.cpu_step(),
            None => {
                // Halt mode - system is in a low-power mode, only (IE and IF) can release CPU from this state.
                if self.io_devs.intc.irq_pending() {
                    self.cpu_interrupt();
                } else {
                    // Fast-forward to next pending HW event so we don't waste time idle-looping when we know the only way
                    // To get out of Halt mode is through an interrupt.
                    self.scheduler.fast_forward_to_next();
                }
            }
        }
    }

    /// Runs the emulation for a given amount of cycles
    /// @return number of cycle actually ran
    #[inline]
    pub(super) fn run<const CHECK_BREAKPOINTS: bool>(&mut self, cycles_to_run: usize) -> usize {
        let start_time = self.scheduler.timestamp();
        let end_time = start_time + cycles_to_run;

        // Register an event to mark the end of this run
        self.scheduler
            .schedule_at(EventType::RunLimitReached, end_time);

        'running: loop {
            // The tricky part is to avoid unnecessary calls for Scheduler::handle_events,
            // performance-wise it would be best to run as many cycles as fast as possible while we know there are no pending events.
            // Safety: Since we pushed a RunLimitReached event, we know this check has a hard limit
            while self.scheduler.timestamp()
                <= unsafe { self.scheduler.timestamp_of_next_event_unchecked() }
            {
                self.single_step();
                if CHECK_BREAKPOINTS {
                    if let Some(bp) = self.cpu.check_breakpoint() {
                        debug!("Arm7tdmi breakpoint hit 0x{:08x}", bp);
                        self.scheduler.cancel_pending(EventType::RunLimitReached);
                        let _ = self.handle_events();
                        if let Some(debugger) = &mut self.debugger {
                            debugger.notify_breakpoint(bp);
                        }
                        break 'running;
                    }
                }
            }

            if self.handle_events() {
                break 'running;
            }
        }

        self.scheduler.timestamp() - start_time
    }

    /// Handle all pending scheduler events and return if run limit was reached.
    #[inline]
    pub(super) fn handle_events(&mut self) -> bool {
        let io = &mut (*self.io_devs);
        while let Some((event, event_time)) = self.scheduler.pop_pending_event() {
            // Since we only examine the scheduler queue every so often, most events will be handled late by a few cycles.
            // We sacrifice accuricy in favor of performance, otherwise we would have to check the event queue
            // every cpu cycle, where in 99% of cases it will always be empty.
            let new_event = match event {
                EventType::RunLimitReached => {
                    // If we have pending events, we handle by the next frame.
                    return true;
                }
                EventType::DmaActivateChannel(channel_id) => {
                    io.dmac.activate_channel(channel_id);
                    None
                }
                EventType::TimerOverflow(channel_id) => {
                    let timers = &mut io.timers;
                    let dmac = &mut io.dmac;
                    let apu = &mut io.sound;
                    Some(timers.handle_overflow_event(channel_id, event_time, apu, dmac))
                }
                EventType::Gpu(gpu_event) => Some(io.gpu.on_event(gpu_event, &mut *self.sysbus)),
                EventType::Apu(event) => Some(io.sound.on_event(event, &mut self.audio_interface)),
            };
            if let Some((new_event, when)) = new_event {
                // We schedule events added by event handlers relative to the handled event time
                self.scheduler.schedule_at(new_event, event_time + when)
            }
        }
        false
    }

    pub fn skip_bios(&mut self) {
        self.cpu.banks.gpr_banked_r13[0] = 0x0300_7f00; // USR/SYS
        self.cpu.banks.gpr_banked_r13[1] = 0x0300_7f00; // FIQ
        self.cpu.banks.gpr_banked_r13[2] = 0x0300_7fa0; // IRQ
        self.cpu.banks.gpr_banked_r13[3] = 0x0300_7fe0; // SVC
        self.cpu.banks.gpr_banked_r13[4] = 0x0300_7f00; // ABT
        self.cpu.banks.gpr_banked_r13[5] = 0x0300_7f00; // UND
        self.cpu.gpr[13] = 0x0300_7f00;
        self.cpu.pc = 0x0800_0000;
        self.cpu.cpsr.set(0x5f);
        self.sysbus.io.gpu.skip_bios();
    }

    #[cfg(feature = "debugger")]
    pub fn add_breakpoint(&mut self, addr: u32) -> Option<usize> {
        let breakpoints = &mut self.cpu.dbg.breakpoints;
        if !breakpoints.contains(&addr) {
            let new_index = breakpoints.len();
            breakpoints.push(addr);
            Some(new_index)
        } else {
            None
        }
    }

    #[cfg(feature = "debugger")]
    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.cpu.get_next_pc();
        for bp in &self.cpu.dbg.breakpoints {
            if (*bp & !1) == next_pc {
                return Some(*bp);
            }
        }

        None
    }

    pub fn get_frame_buffer(&self) -> &[u32] {
        self.sysbus.io.gpu.get_frame_buffer()
    }

    /// Reset the emulator
    pub fn soft_reset(&mut self) {
        self.cpu.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::prelude::*;

    fn make_mock_gba(rom: &[u8]) -> GameBoyAdvance {
        let bios = vec![0; 0x4000].into_boxed_slice();
        let cartridge = GamepakBuilder::new()
            .buffer(rom)
            .with_sram()
            .without_backup_to_file()
            .build()
            .unwrap();
        let mut gba = GameBoyAdvance::new(bios, cartridge, NullAudio::new());
        gba.skip_bios();

        gba
    }

    #[test]
    fn test_arm7tdmi_arm_eggvance() {
        let mut gba = make_mock_gba(include_bytes!("../../external/gba-suite/arm/arm.gba"));

        for _ in 0..10 {
            gba.frame();
        }

        let insn = gba.sysbus.read_32(gba.cpu.pc - 8);
        assert_eq!(insn, 0xeafffffe); // loop
        assert_eq!(0, gba.cpu.gpr[12]);
    }

    #[test]
    fn test_arm7tdmi_thumb_eggvance() {
        let mut gba = make_mock_gba(include_bytes!("../../external/gba-suite/thumb/thumb.gba"));

        for _ in 0..10 {
            gba.frame();
        }

        let insn = gba.sysbus.read_16(gba.cpu.pc - 4);
        assert_eq!(insn, 0xe7fe); // loop
        assert_eq!(0, gba.cpu.gpr[7]);
    }
}
