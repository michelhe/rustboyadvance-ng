use super::arm7tdmi::{exception::Exception, Core};

use crate::bit::BitIndex;

#[derive(Debug, Primitive, Copy, Clone, PartialEq)]
#[allow(non_camel_case_types)]
pub enum Interrupt {
    LCD_VBlank = 0,
    LCD_HBlank = 1,
    LCD_VCounterMatch = 2,
    Timer0_Overflow = 3,
    Timer1_Overflow = 4,
    Timer2_Overflow = 5,
    Timer3_Overflow = 6,
    SerialCommunication = 7,
    DMA0 = 8,
    DMA1 = 9,
    DMA2 = 10,
    DMA3 = 11,
    Keypad = 12,
    GamePak = 13,
}

#[derive(Debug)]
pub struct InterruptController {
    pub interrupt_master_enable: bool,
    pub interrupt_enable: u16,
    pub interrupt_flags: u16,
}

impl InterruptController {
    pub fn new() -> InterruptController {
        InterruptController {
            interrupt_master_enable: false,
            interrupt_enable: 0,
            interrupt_flags: 0,
        }
    }

    pub fn interrupts_disabled(&self, cpu: &Core) -> bool {
        cpu.cpsr.irq_disabled() | (self.interrupt_master_enable)
    }

    pub fn request_irq(&mut self, cpu: &mut Core, irq: Interrupt) {
        if self.interrupts_disabled(cpu) {
            return;
        }
        let irq_bit_index = irq as usize;
        if self.interrupt_enable.bit(irq_bit_index) {
            self.interrupt_flags = 1 << irq_bit_index;
            cpu.exception(Exception::Irq);
        }
    }
}
