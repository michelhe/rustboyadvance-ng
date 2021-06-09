use bit::BitIndex;
use bit_reverse::LookupReverse;
use chrono::prelude::*;
use serde::{Deserialize, Serialize};

use num::FromPrimitive;

use std::cmp;

use super::gpio::{GpioDevice, GpioDirection, GpioState};

fn num2bcd(mut num: u8) -> u8 {
    num = cmp::min(num, 99);

    let mut bcd = 0;
    let mut digit = 1;

    while num > 0 {
        let x = num % 10;
        bcd += x * digit;
        digit = digit << 4;
        num /= 10;
    }
    bcd
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
enum Port {
    /// Serial Clock
    Sck = 0,
    /// Serial IO
    Sio = 1,
    /// Chip Select
    Cs = 2,
}

impl Port {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Struct holding the logical state of a serial port
#[repr(transparent)]
#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
struct PortValue(u16);

#[allow(dead_code)]
impl PortValue {
    fn get(&self) -> u16 {
        self.0
    }

    fn set(&mut self, value: u16) {
        self.0 = value & 1;
    }

    fn high(&self) -> bool {
        self.0 != 0
    }

    fn low(&self) -> bool {
        self.0 == 0
    }
}

/// RTC Registers codes in the GBA
#[derive(Primitive, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
enum RegisterKind {
    ForceReset = 0,
    DateTime = 2,
    ForceIrq = 3,
    Status = 4,
    Time = 6,
    Free = 7,
}

impl RegisterKind {
    fn param_count(&self) -> u8 {
        use RegisterKind::*;
        match self {
            ForceReset => 0,
            DateTime => 7,
            ForceIrq => 0,
            Status => 1,
            Time => 3,
            Free => 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
enum RtcState {
    Idle,
    WaitForChipSelectHigh,
    GetCommandByte,
    RxFromMaster {
        reg: RegisterKind,
        byte_count: u8,
        byte_index: u8,
    },
    TxToMaster {
        byte_count: u8,
        byte_index: u8,
    },
}

/// A Simple LSB-first 8-bit queue
#[derive(Serialize, Deserialize, Clone, DebugStub)]
struct SerialBuffer {
    byte: u8,
    counter: usize,
}

impl SerialBuffer {
    fn new() -> Self {
        SerialBuffer {
            byte: 0,
            counter: 0,
        }
    }

    #[inline]
    fn push_bit(&mut self, bit: bool) {
        if self.counter == 8 {
            return;
        }
        self.byte.set_bit(self.counter, bit);
        self.counter += 1;
    }

    #[inline]
    fn pop_bit(&mut self) -> Option<bool> {
        if self.counter > 0 {
            let result = self.byte.bit(0);
            self.byte = self.byte.wrapping_shr(1);
            self.counter -= 1;
            Some(result)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.byte = 0;
        self.counter = 0;
    }

    fn count(&self) -> usize {
        self.counter
    }

    fn is_empty(&self) -> bool {
        self.count() == 0
    }

    fn is_full(&self) -> bool {
        self.count() == 8
    }

    fn value(&self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            let mask = if self.is_full() {
                0b11111111
            } else {
                (1 << self.counter) - 1
            };
            Some(self.byte & u8::from(mask))
        }
    }

    fn load_byte(&mut self, value: u8) {
        self.byte = value;
        self.counter = 8;
    }

    fn take_byte(&mut self) -> Option<u8> {
        let result = self.value();
        self.reset();
        result
    }
}

/// Model of the S3511 8pin RTC
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rtc {
    state: RtcState,
    sck: PortValue,
    sio: PortValue,
    cs: PortValue,
    status: registers::StatusRegister,
    serial_buffer: SerialBuffer,
    internal_buffer: [u8; 8],
}

impl Rtc {
    pub fn new() -> Self {
        Rtc {
            state: RtcState::Idle,
            sck: PortValue(0),
            sio: PortValue(0),
            cs: PortValue(0),
            status: registers::StatusRegister(0x82),
            serial_buffer: SerialBuffer::new(),
            internal_buffer: [0; 8],
        }
    }

    fn serial_read(&mut self) {
        self.serial_buffer.push_bit(self.sio.high());
    }

    fn force_reset(&mut self) {
        self.serial_buffer.reset();
        self.state = RtcState::Idle;
        self.status.write(0);
        // TODO according to the S3511 datasheet,
        // the date time registers should be reset to 0-0-0-0..
    }

    fn serial_transfer_in_progress(&self) -> bool {
        use RtcState::*;
        match self.state {
            Idle | WaitForChipSelectHigh => false,
            _ => true,
        }
    }

    /// Loads a register contents into an internal buffer
    fn load_register(&mut self, r: RegisterKind) {
        match r {
            RegisterKind::Status => self.internal_buffer[0] = self.status.read(),
            RegisterKind::DateTime => {
                let local: DateTime<Local> = Local::now();
                let year = local.year();
                assert!(year >= 2000 && year <= 2099); // Wonder if I will live to see this one fail

                let hour = if self.status.mode_24h() {
                    local.hour()
                } else {
                    let (_, hour12) = local.hour12();
                    hour12 - 1
                };

                self.internal_buffer[0] = num2bcd((year % 100) as u8);
                self.internal_buffer[1] = num2bcd(local.month() as u8);
                self.internal_buffer[2] = num2bcd(local.day() as u8);
                self.internal_buffer[3] = num2bcd(local.weekday().number_from_monday() as u8);
                self.internal_buffer[4] = num2bcd(hour as u8);
                self.internal_buffer[5] = num2bcd(local.minute() as u8);
                self.internal_buffer[6] = num2bcd(local.second() as u8);
            }
            RegisterKind::Time => {
                let local: DateTime<Local> = Local::now();
                let hour = if self.status.mode_24h() {
                    local.hour()
                } else {
                    let (_, hour12) = local.hour12();
                    hour12 - 1
                };
                self.internal_buffer[0] = num2bcd(hour as u8);
                self.internal_buffer[1] = num2bcd(local.minute() as u8);
                self.internal_buffer[2] = num2bcd(local.second() as u8);
            }
            _ => warn!("RTC: read {:?} not implemented", r),
        }
    }

    fn store_register(&mut self, r: RegisterKind) {
        use RegisterKind::*;
        match r {
            Status => self.status.write(self.internal_buffer[0]),
            ForceReset => self.force_reset(),
            _ => warn!("RTC: write {:?} not implemented", r),
        }
    }
}

impl GpioDevice for Rtc {
    fn write(&mut self, gpio_state: &GpioState, data: u16) {
        assert_eq!(gpio_state[Port::Sck.index()], GpioDirection::Out);
        assert_eq!(gpio_state[Port::Cs.index()], GpioDirection::Out);

        let old_sck = self.sck;
        let old_cs = self.cs;

        self.sck.set(data.bit(Port::Sck.index()) as u16);
        self.cs.set(data.bit(Port::Cs.index()) as u16);

        let sck_falling_edge = old_sck.high() && self.sck.low();

        if sck_falling_edge && gpio_state[Port::Sio.index()] == GpioDirection::Out {
            self.sio.set(data.bit(Port::Sio.index()) as u16);
        }

        if self.cs.high() && old_cs.low() {
            trace!("RTC: CS went from low to high!");
        }

        use RtcState::*;

        if self.cs.low() && self.serial_transfer_in_progress() {
            debug!(
                "RTC: CS set low from state {:?}, resetting state",
                self.state
            );
            self.serial_buffer.reset();
            self.state = Idle;
            return;
        }

        match self.state {
            Idle => {
                if self.sck.high() && self.cs.low() {
                    if self.cs.low() {
                        self.state = WaitForChipSelectHigh;
                    } else {
                        self.state = GetCommandByte;
                    }
                }
            }
            WaitForChipSelectHigh => {
                if self.sck.high() && self.cs.high() {
                    self.state = GetCommandByte;
                    self.serial_buffer.reset();
                }
            }
            GetCommandByte => {
                if !sck_falling_edge {
                    return;
                }

                // receive bit
                self.serial_read();
                if !self.serial_buffer.is_full() {
                    return;
                }

                // finished collecting all the bits
                let mut command = self.serial_buffer.value().unwrap();
                self.serial_buffer.reset();

                let lsb_first = command.bit_range(0..4) == 0b0110;
                if !lsb_first && command.bit_range(4..8) != 0b0110 {
                    panic!("RTC bad command format");
                }

                if !lsb_first {
                    command = command.swap_bits();
                }

                let reg = RegisterKind::from_u8(command.bit_range(4..7)).expect("RTC bad register");
                let byte_count = reg.param_count();

                let is_read_operation = command.bit(7);

                debug!(
                    "RTC: got command: {} {:?} args len: {}",
                    if is_read_operation { "READ" } else { "WRITE" },
                    reg,
                    byte_count
                );

                if byte_count != 0 {
                    if is_read_operation {
                        self.load_register(reg);
                        self.state = TxToMaster {
                            byte_count,
                            byte_index: 0,
                        };
                        self.serial_buffer.reset();
                    } else {
                        self.state = RxFromMaster {
                            reg,
                            byte_count,
                            byte_index: 0,
                        };
                        self.serial_buffer.reset();
                    }
                } else {
                    assert!(!is_read_operation);
                    self.store_register(reg);
                    self.state = Idle;
                    self.serial_buffer.reset();
                }
            }
            TxToMaster {
                byte_count,
                byte_index,
            } => {
                if !sck_falling_edge {
                    return;
                }

                let mut new_byte_index = byte_index;
                let bit = if let Some(bit) = self.serial_buffer.pop_bit() {
                    bit
                } else if byte_index < byte_count {
                    self.serial_buffer
                        .load_byte(self.internal_buffer[byte_index as usize]);
                    new_byte_index += 1;
                    self.serial_buffer.pop_bit().unwrap()
                } else {
                    self.state = Idle;
                    self.serial_buffer.reset();
                    return;
                };

                trace!("RTC TX BIT {}", bit);
                assert_eq!(gpio_state[Port::Sio.index()], GpioDirection::In);
                self.sio.set(bit as u16);

                if self.serial_buffer.is_empty() && new_byte_index == byte_count {
                    self.state = Idle;
                    self.serial_buffer.reset();
                    return;
                }

                self.state = TxToMaster {
                    byte_count,
                    byte_index: new_byte_index,
                };
            }
            RxFromMaster {
                reg,
                byte_count,
                byte_index,
            } => {
                if !sck_falling_edge {
                    return;
                }

                assert_eq!(gpio_state[Port::Sio.index()], GpioDirection::Out);

                self.serial_read();
                if !self.serial_buffer.is_full() {
                    return;
                }

                self.internal_buffer[byte_index as usize] = self.serial_buffer.take_byte().unwrap();

                let byte_index = byte_index + 1;

                if byte_index == byte_count {
                    self.store_register(reg);
                    self.state = Idle;
                } else {
                    self.state = RxFromMaster {
                        reg,
                        byte_count,
                        byte_index,
                    }
                }
            }
        }
    }

    fn read(&self, _gpio_state: &GpioState) -> u16 {
        let mut result = 0;
        result.set_bit(Port::Sck.index(), self.sck.high());
        result.set_bit(Port::Sio.index(), self.sio.high());
        result.set_bit(Port::Cs.index(), self.cs.high());
        result
    }
}

mod registers {

    bitfield! {
        #[derive(Serialize, Deserialize, Clone)]
        pub struct StatusRegister(u8);
        impl Debug;
        u8;
        pub intfe, set_intfe : 1; // unimplemented
        pub intme, set_intme : 3; // unimplemented
        pub intae, set_intae : 5; // unimplemented
        pub mode_24h, set_mode_24h : 6;
        pub power_fail, set_power_fail : 7;
    }

    impl StatusRegister {
        const IGNORED_MASK: u8 = 0b1001_0101;
        pub(super) fn read(&self) -> u8 {
            self.0
        }

        pub(super) fn write(&mut self, value: u8) {
            self.0 = !Self::IGNORED_MASK & value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn transmit(rtc: &mut Rtc, gpio_state: &GpioState, bit: u8) {
        rtc.write(&gpio_state, 0b0100_u16 | (u16::from(bit) << 1));
        rtc.write(&gpio_state, 0b0101_u16);
    }

    fn receive_bytes(rtc: &mut Rtc, gpio_state: &GpioState, bytes: &mut [u8]) {
        for byte in bytes.iter_mut() {
            for i in 0..8 {
                rtc.write(&gpio_state, 0b0100_u16);
                let data = rtc.read(&gpio_state);
                rtc.write(&gpio_state, 0b0101_u16);
                byte.set_bit(i, data.bit(Port::Sio.index()));
            }
        }
    }

    fn transmit_bits(rtc: &mut Rtc, gpio_state: &GpioState, bits: &[u8]) {
        for bit in bits.iter() {
            transmit(rtc, gpio_state, *bit);
        }
    }

    fn start_serial_transfer(rtc: &mut Rtc, gpio_state: &GpioState) {
        assert_eq!(rtc.state, RtcState::Idle);

        // set CS low,
        rtc.write(&gpio_state, 0b0001);
        assert_eq!(rtc.state, RtcState::WaitForChipSelectHigh);

        // set CS high, SCK rising edge
        rtc.write(&gpio_state, 0b0101);
        assert_eq!(rtc.state, RtcState::GetCommandByte);
    }

    #[test]
    fn test_serial_buffer() {
        let mut serial_buffer = SerialBuffer::new();
        assert_eq!(serial_buffer.value(), None);
        serial_buffer.push_bit(true);
        assert_eq!(serial_buffer.value(), Some(1));
        let _ = serial_buffer.pop_bit();
        assert_eq!(serial_buffer.value(), None);

        serial_buffer.push_bit(false);
        serial_buffer.push_bit(true);
        serial_buffer.push_bit(true);
        serial_buffer.push_bit(false);
        assert_eq!(serial_buffer.count(), 4);

        assert_eq!(serial_buffer.value(), Some(6));
        let _ = serial_buffer.pop_bit();
        assert_eq!(serial_buffer.value(), Some(3)); // pops are LSB first
        let _ = serial_buffer.pop_bit();
        assert_eq!(serial_buffer.count(), 2);
        assert_eq!(serial_buffer.value(), Some(1));
    }

    macro_rules! setup_rtc {
        ($rtc:ident, $gpio_state:ident) => {
            let mut $rtc = Rtc::new();
            #[allow(unused_mut)]
            let mut $gpio_state = [
                GpioDirection::Out, /* SCK */
                GpioDirection::Out, /* SIO */
                GpioDirection::Out, /* CS */
                GpioDirection::In,  /* dont-care */
            ];
        };
    }

    #[test]
    fn test_rtc_status() {
        setup_rtc!(rtc, gpio_state);

        rtc.status.set_mode_24h(false);
        start_serial_transfer(&mut rtc, &mut gpio_state);

        // write Status register command
        transmit_bits(&mut rtc, &gpio_state, &[0, 1, 1, 0, 0, 0, 1, 0]);
        assert_eq!(
            rtc.state,
            RtcState::RxFromMaster {
                reg: RegisterKind::Status,
                byte_count: 1,
                byte_index: 0,
            }
        );

        assert_eq!(rtc.status.mode_24h(), false);

        let mut serial_buffer = SerialBuffer::new();
        serial_buffer.load_byte(1 << 6);

        while let Some(bit) = serial_buffer.pop_bit() {
            transmit(&mut rtc, &gpio_state, bit as u8);
        }

        assert!(rtc.serial_buffer.is_empty());
        assert_eq!(rtc.status.mode_24h(), true);

        start_serial_transfer(&mut rtc, &mut gpio_state);

        // read Status register command
        transmit_bits(&mut rtc, &gpio_state, &[0, 1, 1, 0, 0, 0, 1, 1]);
        assert_eq!(
            rtc.state,
            RtcState::TxToMaster {
                byte_count: 1,
                byte_index: 0,
            }
        );

        // adjust SIO pin
        gpio_state[Port::Sio.index()] = GpioDirection::In;

        let mut bytes = [0];
        receive_bytes(&mut rtc, &gpio_state, &mut bytes);

        let mut read_status = registers::StatusRegister(bytes[0]);
        assert_eq!(read_status.mode_24h(), true);
    }

    #[test]
    fn test_date_time() {
        setup_rtc!(rtc, gpio_state);

        start_serial_transfer(&mut rtc, &mut gpio_state);
        transmit_bits(&mut rtc, &gpio_state, &[0, 1, 1, 0, 0, 1, 0, 1]);
        assert_eq!(
            rtc.state,
            RtcState::TxToMaster {
                byte_count: 7,
                byte_index: 0
            }
        );

        gpio_state[Port::Sio.index()] = GpioDirection::In;
        let mut bytes = [0; 7];
        receive_bytes(&mut rtc, &gpio_state, &mut bytes);
        assert_eq!(rtc.state, RtcState::Idle);

        println!("{:x?}", bytes);
        let local: DateTime<Local> = Local::now();
        assert_eq!(bytes[0], num2bcd((local.year() % 100) as u8));
        assert_eq!(bytes[1], num2bcd(local.month() as u8));
        assert_eq!(bytes[2], num2bcd(local.day() as u8));
    }
}
