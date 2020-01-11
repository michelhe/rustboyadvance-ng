/// Struct containing everything
use std::cell::RefCell;
use std::rc::Rc;

use super::arm7tdmi::Core;
use super::cartridge::Cartridge;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sound::SoundController;
use super::sysbus::SysBus;

use super::super::{AudioInterface, InputInterface, VideoInterface};

pub struct GameBoyAdvance {
    pub sysbus: Box<SysBus>,
    pub cpu: Core,
    input_device: Rc<RefCell<dyn InputInterface>>,

    cycles_to_next_event: usize,
}

impl GameBoyAdvance {
    pub fn new(
        cpu: Core,
        bios_rom: Vec<u8>,
        gamepak: Cartridge,
        video_device: Rc<RefCell<dyn VideoInterface>>,
        audio_device: Rc<RefCell<dyn AudioInterface>>,
        input_device: Rc<RefCell<dyn InputInterface>>,
    ) -> GameBoyAdvance {
        let gpu = Box::new(Gpu::new(video_device));
        let sound_controller = Box::new(SoundController::new(audio_device));
        let io = IoDevices::new(gpu, sound_controller);
        GameBoyAdvance {
            cpu: cpu,
            sysbus: Box::new(SysBus::new(io, bios_rom, gamepak)),
            input_device: input_device,

            cycles_to_next_event: 1,
        }
    }

    #[inline]
    pub fn key_poll(&mut self) {
        self.sysbus.io.keyinput = self.input_device.borrow_mut().poll();
    }

    pub fn frame(&mut self) {
        self.key_poll();
        self.sysbus.io.gpu.clear();
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

    fn step_cpu(&mut self, io: &mut IoDevices) -> usize {
        if io.intc.irq_pending()
            && self.cpu.last_executed.is_some()
            && !self.cpu.did_pipeline_flush()
        {
            self.cpu.irq(&mut self.sysbus);
            io.haltcnt = HaltState::Running;
        }
        let previous_cycles = self.cpu.cycles;
        self.cpu.step(&mut self.sysbus);
        self.cpu.cycles - previous_cycles
    }

    pub fn step(&mut self) {
        // // I hate myself for doing this, but rust left me no choice.
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
        );
        io.sound.update(cycles, &mut cycles_to_next_event);
        self.cycles_to_next_event = cycles_to_next_event;
        io.intc.request_irqs(irqs);
    }
}
