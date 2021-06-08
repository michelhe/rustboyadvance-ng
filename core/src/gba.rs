/// Struct containing everything
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bincode;
use serde::{Deserialize, Serialize};

use super::arm7tdmi;
use super::cartridge::Cartridge;
use super::dma::DmaController;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sched::{EventType, Scheduler, SchedulerConnect, SharedScheduler};
use super::sound::SoundController;
use super::sysbus::SysBus;
use super::timer::Timers;
use super::util::Shared;

#[cfg(not(feature = "no_video_interface"))]
use super::VideoInterface;
use super::{AudioInterface, InputInterface};

pub struct GameBoyAdvance {
    pub cpu: Box<arm7tdmi::Core<SysBus>>,
    pub sysbus: Shared<SysBus>,
    pub io_devs: Shared<IoDevices>,
    pub scheduler: SharedScheduler,
    interrupt_flags: SharedInterruptFlags,
    #[cfg(not(feature = "no_video_interface"))]
    pub video_device: Rc<RefCell<dyn VideoInterface>>,
    pub audio_device: Rc<RefCell<dyn AudioInterface>>,
    pub input_device: Rc<RefCell<dyn InputInterface>>,
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
        #[cfg(not(feature = "no_video_interface"))] video_device: Rc<RefCell<dyn VideoInterface>>,
        audio_device: Rc<RefCell<dyn AudioInterface>>,
        input_device: Rc<RefCell<dyn InputInterface>>,
    ) -> GameBoyAdvance {
        // Warn the user if the bios is not the real one
        match check_real_bios(&bios_rom) {
            true => info!("Verified bios rom"),
            false => warn!("This is not the real bios rom, some games may not be compatible"),
        };

        let interrupt_flags = Rc::new(Cell::new(IrqBitmask(0)));
        let scheduler = Scheduler::new_shared();

        let intc = InterruptController::new(interrupt_flags.clone());
        let gpu = Box::new(Gpu::new(scheduler.clone(), interrupt_flags.clone()));
        let dmac = DmaController::new(interrupt_flags.clone(), scheduler.clone());
        let timers = Timers::new(interrupt_flags.clone(), scheduler.clone());
        let sound_controller = Box::new(SoundController::new(
            scheduler.clone(),
            audio_device.borrow().get_sample_rate() as f32,
        ));
        let io_devs = Shared::new(IoDevices::new(intc, gpu, dmac, timers, sound_controller));
        let sysbus = Shared::new(SysBus::new(
            scheduler.clone(),
            io_devs.clone(),
            bios_rom,
            gamepak,
        ));

        let cpu = Box::new(arm7tdmi::Core::new(sysbus.clone()));

        let mut gba = GameBoyAdvance {
            cpu,
            sysbus,
            io_devs,

            #[cfg(not(feature = "no_video_interface"))]
            video_device: video_device,
            audio_device: audio_device,
            input_device: input_device,

            scheduler: scheduler,

            interrupt_flags: interrupt_flags,
        };

        gba.sysbus.init(gba.cpu.weak_ptr());

        gba
    }

    pub fn from_saved_state(
        savestate: &[u8],
        bios: Box<[u8]>,
        rom: Box<[u8]>,
        #[cfg(not(feature = "no_video_interface"))] video_device: Rc<RefCell<dyn VideoInterface>>,
        audio_device: Rc<RefCell<dyn AudioInterface>>,
        input_device: Rc<RefCell<dyn InputInterface>>,
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
        let mut arm7tdmi = Box::new(arm7tdmi::Core::from_saved_state(
            sysbus.clone(),
            decoded.cpu_state,
        ));

        sysbus.init(arm7tdmi.weak_ptr());

        Ok(GameBoyAdvance {
            cpu: arm7tdmi,
            sysbus: sysbus,
            io_devs,

            interrupt_flags: interrupts,

            #[cfg(not(feature = "no_video_interface"))]
            video_device: video_device,
            audio_device: audio_device,
            input_device: input_device,

            scheduler,
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
        self.io_devs.connect_scheduler(self.scheduler.clone());
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
    pub fn key_poll(&mut self) {
        self.sysbus.io.keyinput = self.input_device.borrow_mut().poll();
    }

    pub fn frame(&mut self) {
        self.key_poll();
        static mut OVERSHOOT: usize = 0;
        unsafe {
            OVERSHOOT = self.run(CYCLES_FULL_REFRESH - OVERSHOOT);
        }
    }

    #[inline]
    fn dma_step(&mut self) {
        self.io_devs.dmac.perform_work(&mut self.sysbus);
    }

    #[inline]
    pub fn cpu_step(&mut self) {
        if self.io_devs.intc.irq_pending() {
            self.cpu.irq();
            self.io_devs.haltcnt = HaltState::Running;
        }
        self.cpu.step();
    }

    #[inline]
    fn get_bus_master(&mut self) -> Option<BusMaster> {
        match (self.io_devs.dmac.is_active(), self.io_devs.haltcnt) {
            (true, _) => Some(BusMaster::Dma),
            (false, HaltState::Running) => Some(BusMaster::Cpu),
            (false, _) => None,
        }
    }

    /// Runs the emulation for a given amount of cycles
    /// @return number of extra cycle ran in this iteration
    #[inline]
    fn run(&mut self, cycles_to_run: usize) -> usize {
        let run_start_time = self.scheduler.timestamp();

        // Register an event to mark the end of this run
        self.scheduler
            .push(EventType::RunLimitReached, cycles_to_run);

        let mut running = true;
        while running {
            // The tricky part is to avoid unnecessary calls for Scheduler::process_pending,
            // performance-wise it would be best to run as many cycles as fast as possible while we know there are no pending events.
            // Fast forward emulation until an event occurs
            while self.scheduler.timestamp() <= self.scheduler.timestamp_of_next_event() {
                // 3 Options:
                // 1. DMA is active - thus CPU is blocked
                // 2. DMA inactive and halt state is RUN - CPU can run
                // 3. DMA inactive and halt state is HALT - CPU is blocked
                match self.get_bus_master() {
                    Some(BusMaster::Dma) => self.dma_step(),
                    Some(BusMaster::Cpu) => self.cpu_step(),
                    None => {
                        if self.io_devs.intc.irq_pending() {
                            self.io_devs.haltcnt = HaltState::Running;
                        } else {
                            self.scheduler.fast_forward_to_next();
                            let (event, cycles_late) = self
                                .scheduler
                                .pop_pending_event()
                                .unwrap_or_else(|| unreachable!());
                            self.handle_event(event, cycles_late, &mut running);
                        }
                    }
                }
            }

            while let Some((event, cycles_late)) = self.scheduler.pop_pending_event() {
                self.handle_event(event, cycles_late, &mut running);
            }
        }

        let total_cycles_ran = self.scheduler.timestamp() - run_start_time;
        total_cycles_ran - cycles_to_run
    }

    #[inline]
    fn handle_event(&mut self, event: EventType, cycles_late: usize, running: &mut bool) {
        let io = &mut (*self.io_devs);
        match event {
            EventType::RunLimitReached => {
                *running = false;
            }
            EventType::DmaActivateChannel(channel_id) => io.dmac.activate_channel(channel_id),
            EventType::TimerOverflow(channel_id) => {
                let timers = &mut io.timers;
                let dmac = &mut io.dmac;
                let apu = &mut io.sound;
                timers.handle_overflow_event(channel_id, cycles_late, apu, dmac);
            }
            EventType::Gpu(event) => io.gpu.on_event(
                event,
                cycles_late,
                &mut *self.sysbus,
                #[cfg(not(feature = "no_video_interface"))]
                &self.video_device,
            ),
            EventType::Apu(event) => io.sound.on_event(event, cycles_late, &self.audio_device),
        }
    }

    pub fn skip_bios(&mut self) {
        self.cpu.skip_bios();
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

    #[cfg(feature = "debugger")]
    /// 'step' function that checks for breakpoints
    /// TODO avoid code duplication
    pub fn step_debugger(&mut self) -> Option<u32> {
        // clear any pending DMAs
        self.dma_step();

        // Run the CPU
        self.cpu_step();

        let breakpoint = self.check_breakpoint();

        let mut _running = true;
        while let Some((event, cycles_late)) = self.scheduler.pop_pending_event() {
            self.handle_event(event, cycles_late, &mut _running);
        }

        breakpoint
    }

    /// Query the emulator for the recently drawn framebuffer.
    /// for use with implementations where the VideoInterface is not a viable option.
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
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::super::bus::Bus;
    use super::super::cartridge::GamepakBuilder;

    struct DummyInterface {}

    impl DummyInterface {
        fn new() -> DummyInterface {
            DummyInterface {}
        }
    }

    #[cfg(not(feature = "no_video_interface"))]
    impl VideoInterface for DummyInterface {}
    impl AudioInterface for DummyInterface {}
    impl InputInterface for DummyInterface {}

    fn make_mock_gba(rom: &[u8]) -> GameBoyAdvance {
        let bios = vec![0; 0x4000].into_boxed_slice();
        let cartridge = GamepakBuilder::new()
            .buffer(rom)
            .with_sram()
            .without_backup_to_file()
            .build()
            .unwrap();
        let dummy = Rc::new(RefCell::new(DummyInterface::new()));
        let mut gba = GameBoyAdvance::new(
            bios,
            cartridge,
            #[cfg(not(feature = "no_video_interface"))]
            dummy.clone(),
            dummy.clone(),
            dummy.clone(),
        );
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
