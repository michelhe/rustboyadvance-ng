/// Struct containing everything
use std::cell::RefCell;
use std::rc::Rc;

use bincode;
use serde::{Deserialize, Serialize};

use super::arm7tdmi;
use super::cartridge::Cartridge;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sound::SoundController;
use super::sysbus::SysBus;

use super::super::{AudioInterface, InputInterface, VideoInterface};

pub struct GameBoyAdvance {
    pub sysbus: Box<SysBus>,
    pub cpu: arm7tdmi::Core,

    pub video_device: Rc<RefCell<dyn VideoInterface>>,
    pub audio_device: Rc<RefCell<dyn AudioInterface>>,
    pub input_device: Rc<RefCell<dyn InputInterface>>,

    pub cycles_to_next_event: usize,
}

#[derive(Serialize, Deserialize)]
struct SaveState {
    sysbus: Box<SysBus>,
    cpu: arm7tdmi::Core,
}

impl GameBoyAdvance {
    pub fn new(
        bios_rom: Box<[u8]>,
        gamepak: Cartridge,
        video_device: Rc<RefCell<dyn VideoInterface>>,
        audio_device: Rc<RefCell<dyn AudioInterface>>,
        input_device: Rc<RefCell<dyn InputInterface>>,
    ) -> GameBoyAdvance {
        let gpu = Box::new(Gpu::new());
        let sound_controller = Box::new(SoundController::new(
            audio_device.borrow().get_sample_rate() as f32,
        ));
        let io = IoDevices::new(gpu, sound_controller);
        let sysbus = Box::new(SysBus::new(io, bios_rom, gamepak));

        let cpu = arm7tdmi::Core::new();

        let mut gba = GameBoyAdvance {
            cpu: cpu,
            sysbus: sysbus,

            video_device: video_device,
            audio_device: audio_device,
            input_device: input_device,

            cycles_to_next_event: 1,
        };

        gba.sysbus.created();

        gba
    }

    pub fn save_state(&self) -> bincode::Result<Vec<u8>> {
        let s = SaveState {
            cpu: self.cpu.clone(),
            sysbus: self.sysbus.clone(),
        };

        bincode::serialize(&s)
    }

    pub fn restore_state(&mut self, bytes: &[u8]) -> bincode::Result<()> {
        let decoded: Box<SaveState> = bincode::deserialize_from(bytes)?;

        self.cpu = decoded.cpu;
        self.sysbus = decoded.sysbus;
        self.cycles_to_next_event = 1;

        self.sysbus.created();

        Ok(())
    }

    #[inline]
    pub fn key_poll(&mut self) {
        self.sysbus.io.keyinput = self.input_device.borrow_mut().poll();
    }

    pub fn frame(&mut self) {
        self.key_poll();
        while self.sysbus.io.gpu.vcount != DISPLAY_HEIGHT {
            self.step();
        }
        while self.sysbus.io.gpu.vcount == DISPLAY_HEIGHT {
            self.step();
        }
    }

    pub fn add_breakpoint(&mut self, addr: u32) -> Option<usize> {
        if !self.cpu.breakpoints.contains(&addr) {
            let new_index = self.cpu.breakpoints.len();
            self.cpu.breakpoints.push(addr);
            Some(new_index)
        } else {
            None
        }
    }

    pub fn check_breakpoint(&self) -> Option<u32> {
        let next_pc = self.cpu.get_next_pc();
        for bp in &self.cpu.breakpoints {
            if *bp == next_pc {
                return Some(next_pc);
            }
        }

        None
    }

    pub fn skip_bios(&mut self) {
        self.cpu.skip_bios();
        self.sysbus.io.gpu.skip_bios();
    }

    pub fn step_cpu(&mut self, io: &mut IoDevices) -> usize {
        if io.intc.irq_pending() {
            self.cpu.irq(&mut self.sysbus);
            io.haltcnt = HaltState::Running;
        }
        let previous_cycles = self.cpu.cycles;
        self.cpu.step(&mut self.sysbus);
        self.cpu.cycles - previous_cycles
    }

    pub fn step(&mut self) {
        // I hate myself for doing this, but rust left me no choice.
        let io = unsafe {
            let ptr = &mut *self.sysbus as *mut SysBus;
            &mut (*ptr).io as &mut IoDevices
        };

        let mut irqs = IrqBitmask(0);

        let mut cycles_left = self.cycles_to_next_event;
        let mut cycles_to_next_event = std::usize::MAX;
        let mut cycles = 0;

        while cycles_left > 0 {
            let mut irqs = IrqBitmask(0);
            let _cycles = if !io.dmac.is_active() {
                if HaltState::Running == io.haltcnt {
                    self.step_cpu(io)
                } else {
                    cycles = cycles_left;
                    break;
                }
            } else {
                io.dmac.perform_work(&mut self.sysbus, &mut irqs);
                io.intc.request_irqs(irqs);
                return;
            };

            cycles += _cycles;
            if cycles_left < _cycles {
                break;
            }
            cycles_left -= _cycles;
        }

        // update gpu & sound
        io.timers.update(cycles, &mut self.sysbus, &mut irqs);
        io.gpu.step(
            cycles,
            &mut self.sysbus,
            &mut irqs,
            &mut cycles_to_next_event,
            &self.video_device,
        );
        io.sound
            .update(cycles, &mut cycles_to_next_event, &self.audio_device);
        self.cycles_to_next_event = cycles_to_next_event;
        io.intc.request_irqs(irqs);
    }

    /// Query the emulator for the recently drawn framebuffer.
    /// for use with implementations where the VideoInterface is not a viable option.
    pub fn get_frame_buffer(&self) -> &[u32] {
        self.sysbus.io.gpu.get_frame_buffer()
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
        let mut gba =
            GameBoyAdvance::new(bios, cartridge, dummy.clone(), dummy.clone(), dummy.clone());
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
