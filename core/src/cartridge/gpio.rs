use super::{Cartridge, GPIO_PORT_CONTROL, GPIO_PORT_DATA, GPIO_PORT_DIRECTION};
use bit::BitIndex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
pub enum GpioDirection {
    /// GPIO to GBA
    In = 0,
    /// GBA to GPIO
    Out = 1,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
pub enum GpioPortControl {
    WriteOnly = 0,
    ReadWrite = 1,
}

pub trait GpioDevice: Sized {
    fn write(&mut self, gpio_state: &[GpioDirection; 4], data: u16);
    fn read(&self, gpio_state: &[GpioDirection; 4]) -> u16;
}

impl Cartridge {
    pub(super) fn is_gpio_readable(&self) -> bool {
        self.gpio_control != GpioPortControl::WriteOnly
    }

    pub(super) fn gpio_read(&self, addr: u32) -> u16 {
        match addr {
            GPIO_PORT_DATA => {
                if let Some(rtc) = &self.rtc {
                    rtc.read(&self.gpio_direction)
                } else {
                    0
                }
            }
            GPIO_PORT_DIRECTION => {
                let mut direction = 0u16;
                for i in 0..4 {
                    direction.set_bit(i, self.gpio_direction[i] == GpioDirection::Out);
                }
                direction
            }
            GPIO_PORT_CONTROL => self.gpio_control as u16,
            _ => unreachable!(),
        }
    }

    pub(super) fn gpio_write(&mut self, addr: u32, value: u16) {
        match addr {
            GPIO_PORT_DATA => {
                if let Some(rtc) = &mut self.rtc {
                    rtc.write(&self.gpio_direction, value);
                }
            }
            GPIO_PORT_DIRECTION => {
                for i in 0..4 {
                    if value.bit(i) {
                        self.gpio_direction[i] = GpioDirection::Out;
                    } else {
                        self.gpio_direction[i] = GpioDirection::In;
                    }
                }
            }
            GPIO_PORT_CONTROL => {
                self.gpio_control = if value != 0 {
                    GpioPortControl::ReadWrite
                } else {
                    GpioPortControl::WriteOnly
                };
            }
            _ => unreachable!(),
        }
    }
}
