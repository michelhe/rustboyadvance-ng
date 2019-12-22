/// Struct containing everything
use std::cell::RefCell;
use std::rc::Rc;

use super::arm7tdmi::Core;
use super::cartridge::Cartridge;
use super::gpu::*;
use super::interrupt::*;
use super::iodev::*;
use super::sysbus::SysBus;

use super::super::{AudioInterface, InputInterface, VideoInterface};

pub struct GameBoyAdvance {
    pub sysbus: Box<SysBus>,
    pub cpu: Core,
    video_device: Rc<RefCell<dyn VideoInterface>>,
    audio_device: Rc<RefCell<dyn AudioInterface>>,
    input_device: Rc<RefCell<dyn InputInterface>>,
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
        let io = IoDevices::new();
        GameBoyAdvance {
            cpu: cpu,
            sysbus: Box::new(SysBus::new(io, bios_rom, gamepak)),
            video_device: video_device,
            audio_device: audio_device,
            input_device: input_device,
        }
    }

    #[inline]
    pub fn key_poll(&mut self) {
        self.sysbus.io.keyinput = self.input_device.borrow_mut().poll();
    }

    pub fn frame(&mut self) {
        self.key_poll();
        while self.sysbus.io.gpu.state != GpuState::VBlank {
            self.step();
        }
        self.video_device
            .borrow_mut()
            .render(self.sysbus.io.gpu.get_framebuffer());
        while self.sysbus.io.gpu.state == GpuState::VBlank {
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

    pub fn step(&mut self) {
        let mut irqs = IrqBitmask(0);
        let previous_cycles = self.cpu.cycles;

        // // I hate myself for doing this, but rust left me no choice.
        let io = unsafe {
            let ptr = &mut *self.sysbus as *mut SysBus;
            &mut (*ptr).io as &mut IoDevices
        };

        let cycles = if !io.dmac.perform_work(&mut self.sysbus, &mut irqs) {
            if io.intc.irq_pending()
                && self.cpu.last_executed.is_some()
                && !self.cpu.did_pipeline_flush()
            {
                self.cpu.irq(&mut self.sysbus);
                io.haltcnt = HaltState::Running;
            }

            if HaltState::Running == io.haltcnt {
                self.cpu.step(&mut self.sysbus).unwrap();
                self.cpu.cycles - previous_cycles
            } else {
                1
            }
        } else {
            0
        };

        io.timers.step(cycles, &mut self.sysbus, &mut irqs);

        if let Some(new_gpu_state) = io.gpu.step(cycles, &mut self.sysbus, &mut irqs) {
            match new_gpu_state {
                GpuState::VBlank => {
                    io.dmac.notify_vblank();
                }
                GpuState::HBlank => io.dmac.notify_hblank(),
                _ => {}
            }
        }

        io.intc.request_irqs(irqs);
        let mut audio_device = self.audio_device.borrow_mut();
        io.sound.update(self.cpu.cycles, &mut (*audio_device));
    }
}
