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

pub struct InterruptController;
