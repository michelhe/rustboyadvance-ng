use std::cell::Cell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

pub trait InterruptConnect {
    // Connect a SharedInterruptFlags to this interrupt source
    fn connect_irq(&mut self, interrupt_flags: SharedInterruptFlags);
}

#[derive(Serialize, Deserialize, Debug, Primitive, Copy, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct InterruptController {
    pub interrupt_master_enable: bool,
    pub interrupt_enable: IrqBitmask,
    pub interrupt_flags: SharedInterruptFlags,
}

impl InterruptController {
    pub fn new(interrupt_flags: SharedInterruptFlags) -> InterruptController {
        InterruptController {
            interrupt_flags,
            interrupt_master_enable: false,
            ..Default::default()
        }
    }

    #[inline]
    pub fn irq_pending(&self) -> bool {
        self.interrupt_master_enable
            & ((self.interrupt_flags.get().value() & self.interrupt_enable.0) != 0)
    }

    #[inline]
    pub fn clear(&mut self, value: u16) {
        let _if = self.interrupt_flags.get();
        let new_if = _if.0 & !value;
        self.interrupt_flags.set(IrqBitmask(new_if));
    }
}

impl InterruptConnect for InterruptController {
    fn connect_irq(&mut self, interrupt_flags: SharedInterruptFlags) {
        self.interrupt_flags = interrupt_flags;
    }
}

#[inline]
pub fn signal_irq(interrupt_flags: &SharedInterruptFlags, i: Interrupt) {
    let _if = interrupt_flags.get();
    let new_if = _if.0 | 1 << (i as usize);
    interrupt_flags.set(IrqBitmask(new_if));
}

impl IrqBitmask {
    pub fn value(&self) -> u16 {
        self.0
    }
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone, PartialEq)]
    pub struct IrqBitmask(u16);
    impl Debug;
    u16;
    #[allow(non_snake_case)]
    pub LCD_VBlank, set_LCD_VBlank: 0;
    #[allow(non_snake_case)]
    pub LCD_HBlank, set_LCD_HBlank: 1;
    #[allow(non_snake_case)]
    pub LCD_VCounterMatch, set_LCD_VCounterMatch: 2;
    #[allow(non_snake_case)]
    pub Timer0_Overflow, set_Timer0_Overflow: 3;
    #[allow(non_snake_case)]
    pub Timer1_Overflow, set_Timer1_Overflow: 4;
    #[allow(non_snake_case)]
    pub Timer2_Overflow, set_Timer2_Overflow: 5;
    #[allow(non_snake_case)]
    pub Timer3_Overflow, set_Timer3_Overflow: 6;
    #[allow(non_snake_case)]
    pub SerialCommunication, set_SerialCommunication: 7;
    #[allow(non_snake_case)]
    pub DMA0, set_DMA0: 8;
    #[allow(non_snake_case)]
    pub DMA1, set_DMA1: 9;
    #[allow(non_snake_case)]
    pub DMA2, set_DMA2: 10;
    #[allow(non_snake_case)]
    pub DMA3, set_DMA3: 11;
    #[allow(non_snake_case)]
    pub Keypad, set_Keypad: 12;
    #[allow(non_snake_case)]
    pub GamePak, set_GamePak: 13;
}

pub type SharedInterruptFlags = Rc<Cell<IrqBitmask>>;
