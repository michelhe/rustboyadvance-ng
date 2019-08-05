use super::arm7tdmi::Core;

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

#[derive(Debug, Default)]
pub struct InterruptController {
    pub interrupt_master_enable: bool,
    pub interrupt_enable: IrqBitmask,
    pub interrupt_flags: IrqBitmask,
}

impl InterruptController {
    pub fn new() -> InterruptController {
        InterruptController {
            interrupt_master_enable: false,
            ..Default::default()
        }
    }

    pub fn request_irqs(&mut self, flags: IrqBitmask) {
        if !self.interrupt_master_enable {
            return;
        }
        self.interrupt_flags.0 |= flags.0 & self.interrupt_enable.0;
    }

    pub fn irq_pending(&self) -> bool {
        self.interrupt_master_enable & (self.interrupt_flags.0 != 0)
    }
}

impl IrqBitmask {
    pub fn add_irq(&mut self, i: Interrupt) {
        self.0 |= 1 << (i as usize);
    }
}

bitfield! {
    #[derive(Default, Copy, Clone, PartialEq)]
    #[allow(non_snake_case)]
    pub struct IrqBitmask(u16);
    impl Debug;
    u16;
    pub LCD_VBlank, set_LCD_VBlank: 0;
    pub LCD_HBlank, set_LCD_HBlank: 1;
    pub LCD_VCounterMatch, set_LCD_VCounterMatch: 2;
    pub Timer0_Overflow, set_Timer0_Overflow: 3;
    pub Timer1_Overflow, set_Timer1_Overflow: 4;
    pub Timer2_Overflow, set_Timer2_Overflow: 5;
    pub Timer3_Overflow, set_Timer3_Overflow: 6;
    pub SerialCommunication, set_SerialCommunication: 7;
    pub DMA0, set_DMA0: 8;
    pub DMA1, set_DMA1: 9;
    pub DMA2, set_DMA2: 10;
    pub DMA3, set_DMA3: 11;
    pub Keypad, set_Keypad: 12;
    pub GamePak, set_GamePak: 13;
}
