use super::rtc::Rtc;
use super::{GPIO_PORT_CONTROL, GPIO_PORT_DATA, GPIO_PORT_DIRECTION};

use bit::BitIndex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
pub enum GpioDirection {
    /// GPIO to GBA
    In = 0,
    /// GBA to GPIO
    Out = 1,
}

pub type GpioState = [GpioDirection; 4];

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
enum GpioPortControl {
    WriteOnly = 0,
    ReadWrite = 1,
}

pub trait GpioDevice: Sized {
    fn write(&mut self, gpio_state: &GpioState, data: u16);
    fn read(&self, gpio_state: &GpioState) -> u16;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Gpio {
    pub(in crate) rtc: Option<Rtc>,
    direction: GpioState,
    control: GpioPortControl,
}

impl Gpio {
    pub fn new_none() -> Self {
        Gpio {
            rtc: None,
            direction: [GpioDirection::Out; 4],
            control: GpioPortControl::WriteOnly,
        }
    }

    pub fn new_rtc() -> Self {
        Gpio {
            rtc: Some(Rtc::new()),
            direction: [GpioDirection::Out; 4],
            control: GpioPortControl::WriteOnly,
        }
    }

    pub fn is_readable(&self) -> bool {
        self.control != GpioPortControl::WriteOnly
    }

    pub fn read(&self, addr: u32) -> u16 {
        match addr {
            GPIO_PORT_DATA => {
                if let Some(rtc) = &self.rtc {
                    rtc.read(&self.direction)
                } else {
                    0
                }
            }
            GPIO_PORT_DIRECTION => {
                let mut direction = 0u16;
                for i in 0..4 {
                    direction.set_bit(i, self.direction[i] == GpioDirection::Out);
                }
                direction
            }
            GPIO_PORT_CONTROL => self.control as u16,
            _ => unreachable!(),
        }
    }

    pub fn write(&mut self, addr: u32, value: u16) {
        match addr {
            GPIO_PORT_DATA => {
                if let Some(rtc) = &mut self.rtc {
                    rtc.write(&self.direction, value);
                }
            }
            GPIO_PORT_DIRECTION => {
                for i in 0..4 {
                    if value.bit(i) {
                        self.direction[i] = GpioDirection::Out;
                    } else {
                        self.direction[i] = GpioDirection::In;
                    }
                }
            }
            GPIO_PORT_CONTROL => {
                self.control = if value != 0 {
                    GpioPortControl::ReadWrite
                } else {
                    GpioPortControl::WriteOnly
                };
            }
            _ => unreachable!(),
        }
    }
}
