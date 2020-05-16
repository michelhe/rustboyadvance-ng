use super::rtc::Rtc;
use super::{GPIO_PORT_CONTROL, GPIO_PORT_DATA, GPIO_PORT_DIRECTION};

use bit::BitIndex;
use serde::{Deserialize, Serialize};

use std::cell::RefCell;

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
enum GpioDirection {
    In = 0,
    Out = 1,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
enum GpioPortControl {
    WriteOnly = 0,
    ReadWrite = 1,
}

trait GpioDevice: Sized {
    fn write(&mut self);
    fn read(&mut self);
}

enum GpioDeviceType {
    Rtc,
    SolarSensor,
    Gyro,
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Gpio {
    rtc: Option<RefCell<Rtc>>,
    direction: [GpioDirection; 4],
    control: GpioPortControl,
}

impl Gpio {
    pub fn new() -> Self {
        Gpio {
            rtc: None,
            direction: [GpioDirection::In; 4],
            control: GpioPortControl::WriteOnly,
        }
    }

    pub fn is_readable(&self) -> bool {
        self.control != GpioPortControl::WriteOnly
    }

    pub fn read(&self, addr: u32) -> u16 {
        match addr {
            GPIO_PORT_DATA => unimplemented!(),
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

    pub fn write(&mut self, addr: u32) -> u16 {
        match addr {
            GPIO_PORT_DATA => unimplemented!(),
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_gpio() {
        unimplemented!();
    }
}
