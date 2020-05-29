use super::arm7tdmi::CpuState;
use super::core::GameBoyAdvance;
use super::interrupt::*;
use super::iodev::IoDevices;
use super::sysbus::SysBus;
use super::Bus;

use byteorder::{LittleEndian, ReadBytesExt};
use gdbstub::{Access, Target, TargetState};

use std::io::Cursor;

impl Target for GameBoyAdvance {
    type Usize = u32;
    type Error = ();

    fn step(
        &mut self,
        mut _log_mem_access: impl FnMut(Access<u32>),
    ) -> Result<TargetState, Self::Error> {
        static mut S_TOTAL_CYCLES: usize = 0;

        let io = unsafe {
            let ptr = &mut *self.sysbus as *mut SysBus;
            &mut (*ptr).io as &mut IoDevices
        };

        // clear any pending DMAs
        let mut irqs = IrqBitmask(0);
        while io.dmac.is_active() {
            io.dmac.perform_work(&mut self.sysbus, &mut irqs);
        }
        io.intc.request_irqs(irqs);

        // run the CPU, ignore haltcnt
        let cycles = self.step_cpu(io);
        io.timers.update(cycles, &mut self.sysbus, &mut irqs);

        unsafe {
            S_TOTAL_CYCLES += cycles;
        }

        if self.cycles_to_next_event <= unsafe { S_TOTAL_CYCLES } {
            let mut cycles_to_next_event = std::usize::MAX;
            io.gpu.update(
                unsafe { S_TOTAL_CYCLES },
                &mut self.sysbus,
                &mut irqs,
                &mut cycles_to_next_event,
                &self.video_device,
            );
            io.sound.update(
                unsafe { S_TOTAL_CYCLES },
                &mut cycles_to_next_event,
                &self.audio_device,
            );
            self.cycles_to_next_event = cycles_to_next_event;

            unsafe {
                S_TOTAL_CYCLES = 0;
            };
        } else {
            self.cycles_to_next_event -= unsafe { S_TOTAL_CYCLES };
        }

        io.intc.request_irqs(irqs);

        Ok(TargetState::Running)
    }

    fn read_pc(&mut self) -> u32 {
        self.cpu.get_next_pc()
    }

    // read the specified memory addresses from the target
    fn read_addrs(&mut self, addr: std::ops::Range<u32>, mut push_byte: impl FnMut(u8)) {
        for addr in addr {
            push_byte(self.sysbus.read_8(addr))
        }
    }

    // write data to the specified memory addresses
    fn write_addrs(&mut self, mut get_addr_val: impl FnMut() -> Option<(u32, u8)>) {
        while let Some((addr, val)) = get_addr_val() {
            self.sysbus.write_8(addr, val);
        }
    }

    fn read_registers(&mut self, mut push_reg: impl FnMut(&[u8])) {
        // general purpose registers
        for i in 0..15 {
            push_reg(&self.cpu.get_reg(i).to_le_bytes());
        }
        push_reg(&self.cpu.get_next_pc().to_le_bytes());
        // Floating point registers, unused
        for _ in 0..25 {
            push_reg(&[0, 0, 0, 0]);
        }
        push_reg(&self.cpu.cpsr.get().to_le_bytes());
    }

    fn write_registers(&mut self, regs: &[u8]) {
        let mut rdr = Cursor::new(regs);
        for i in 0..15 {
            self.cpu.set_reg(i, rdr.read_u32::<LittleEndian>().unwrap());
        }
        let new_pc = rdr.read_u32::<LittleEndian>().unwrap();
        self.cpu.set_reg(15, new_pc);

        self.cpu.cpsr.set(rdr.read_u32::<LittleEndian>().unwrap());

        match self.cpu.cpsr.state() {
            CpuState::ARM => self.cpu.reload_pipeline32(&mut self.sysbus),
            CpuState::THUMB => self.cpu.reload_pipeline16(&mut self.sysbus),
        };
    }

    fn target_description_xml() -> Option<&'static str> {
        Some(
            r#"
        <target version="1.0">
            <architecture>armv4t</architecture>
        </target>"#,
        )
    }
}
