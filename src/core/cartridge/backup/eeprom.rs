use super::{BackupMemoryInterface};

use num::FromPrimitive;
use serde::{Deserialize, Serialize};

use std::cell::RefCell;

#[derive(Debug)]
pub enum EepromSize {
    Eeprom512,
    Eeprom8k,
}

impl Into<usize> for EepromSize {
    fn into(self) -> usize {
        match self {
            EepromSize::Eeprom512 => 0x0200,
            EepromSize::Eeprom8k => 0x2000,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Primitive, PartialEq, Copy, Clone)]
enum SpiInstruction {
    Read = 0b011,
    Write = 0b010,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
enum SpiState {
    RxInstruction,
    RxAddress(SpiInstruction),
    StopBit(SpiInstruction),
    TxDummy(SpiInstruction),
    TxData(usize),
    RxData(usize),
}

impl Default for SpiState {
    fn default() -> SpiState {
        SpiState::RxInstruction
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct EepromChip<M>
where
    M: BackupMemoryInterface,
{
    memory: M,

    state: SpiState,
    rx_count: usize,
    rx_buffer: u64,

    tx_count: usize,
    tx_buffer: u8,

    address: usize,
}

impl<M> EepromChip<M>
where
    M: BackupMemoryInterface,
{
    fn new(memory: M) -> EepromChip<M> {
        EepromChip {
            memory: memory,
            state: SpiState::RxInstruction,

            rx_count: 0,
            rx_buffer: 0,

            tx_count: 0,
            tx_buffer: 0,

            address: 0,
        }
    }

    fn rx(&mut self, bit: u8) {
        // data is receieved MSB first
        let bit = bit & 1;
        self.rx_buffer = (self.rx_buffer << 1) | (if bit & 1 != 0 { 1 } else { 0 });
        self.rx_count += 1;
    }

    fn tx(&mut self) -> u8 {
        // data is transmitted MSB first
        let bit = self.tx_buffer >> 7;
        self.tx_buffer = self.tx_buffer << 1;
        self.tx_count += 1;

        bit
    }

    fn reset_rx_buffer(&mut self) {
        self.rx_buffer = 0;
        self.rx_count = 0;
    }

    fn reset_tx_buffer(&mut self) {
        self.tx_buffer = 0;
        self.tx_count = 0;
    }

    fn clock_data_in(&mut self, si: u8) {
        use SpiInstruction::*;
        use SpiState::*;

        // Read the si signal into the rx_buffer
        self.rx(si);

        match self.state {
            RxInstruction => {
                // If instruction was recvd, proceed to recv the address
                if self.rx_count >= 2 {
                    let insn = SpiInstruction::from_u64(self.rx_buffer).expect(&format!(
                        "invalid spi command {:#010b}",
                        self.rx_buffer as u8
                    ));
                    self.state = RxAddress(insn);
                    self.reset_rx_buffer();
                }
            }
            RxAddress(insn) => {
                if self.rx_count == 6 {
                    self.address = (self.rx_buffer as usize) * 8;
                    self.state = match insn {
                        Read => StopBit(insn),
                        Write => RxData(0),
                    };
                    self.reset_rx_buffer();
                }
            }
            StopBit(Read) => {
                if si != 0 {
                    panic!(
                        "SPI Read - bit 0 was expected for command termination (debug={:?})",
                        *self
                    );
                }
                self.state = TxDummy(Read);
                self.reset_rx_buffer();
                self.reset_tx_buffer();
            }
            RxData(rx_bytes) => {
                if rx_bytes < 8 {
                    if self.rx_count % 8 == 0 {
                        if rx_bytes + 1 == 8 {
                            self.state = StopBit(Write);
                            self.reset_rx_buffer();
                        } else {
                            let byte = (self.rx_buffer & 0xff) as u8;
                            self.memory.write(self.address, byte);
                            self.reset_rx_buffer();
                            self.address += 1;
                            self.state = RxData(rx_bytes + 1);
                        }
                    }
                }
            }
            StopBit(Write) => {
                if si != 0 {
                    panic!(
                        "SPI Write - bit 0 was expected for command termination (debug={:?})",
                        *self
                    );
                }
                self.state = RxInstruction;
                self.reset_rx_buffer();
                self.reset_tx_buffer();
            }
            _ => {}
        }
    }

    fn clock_data_out(&mut self) -> u8 {
        use SpiState::*;

        match self.state {
            TxDummy(insn) => {
                let bit = self.tx();
                if self.tx_count == 4 {
                    self.state = TxData(0);
                    self.reset_tx_buffer();
                }
                bit
            }
            TxData(tx_bytes) => {
                if tx_bytes < 8 {
                    if self.tx_count % 8 == 0 {
                        let byte = self.memory.read(self.address);
                        self.tx_buffer = byte;
                        self.address += 1;
                        self.state = TxData(tx_bytes + 1);
                    }
                    self.tx()
                } else {
                    self.state = RxInstruction;
                    self.reset_rx_buffer();
                    self.reset_tx_buffer();
                    0
                }
            }
            _ => self.tx(),
        }
    }

    fn data_available(&self) -> bool {
        if let SpiState::TxData(_) = self.state {
            true
        } else {
            false
        }
    }
}

/// The Eeprom controller is usually mapped to the top 256 bytes of the cartridge memory
/// Eeprom controller can programmed with DMA accesses in 16bit mode
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpiController<M>
where
    M: BackupMemoryInterface,
{
    chip: RefCell<EepromChip<M>>,
}

impl<M> SpiController<M>
where
    M: BackupMemoryInterface,
{
    pub fn new(m: M) -> SpiController<M> {
        SpiController {
            chip: RefCell::new(EepromChip::new(m)),
        }
    }

    pub fn write_half(&mut self, value: u16) {
        self.chip.borrow_mut().clock_data_in(value as u8);
    }

    pub fn read_half(&self) -> u16 {
        let mut chip = self.chip.borrow_mut();
        let bit = chip.clock_data_out() as u16;
        if chip.data_available() {
            bit
        } else {
            0
        }
    }

    #[cfg(test)]
    fn consume_dummy_cycles(&self) {
        // ignore the dummy bits
        self.read_half();
        self.read_half();
        self.read_half();
        self.read_half();
    }

    #[cfg(test)]
    fn rx_data(&self) -> [u8; 8] {
        let mut bytes = [0; 8];
        for byte_index in 0..8 {
            let mut byte = 0;
            for _ in 0..8 {
                let bit = self.read_half() as u8;
                byte = (byte << 1) | bit;
            }
            bytes[byte_index] = byte;
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::BackupMemoryInterface;
    use super::*;

    use bit::BitIndex;
    use hexdump;

    use std::io::Write;

    #[derive(Debug)]
    struct MockMemory {
        buffer: Vec<u8>,
    }

    impl BackupMemoryInterface for MockMemory {
        fn write(&mut self, offset: usize, value: u8) {
            self.buffer[offset] = value;
        }

        fn read(&self, offset: usize) -> u8 {
            self.buffer[offset]
        }
    }

    fn make_mock_memory() -> MockMemory {
        let mut buffer = vec![0; 512];
        buffer[16] = 'T' as u8;
        buffer[17] = 'E' as u8;
        buffer[18] = 'S' as u8;
        buffer[19] = 'T' as u8;
        buffer[20] = '!' as u8;

        MockMemory { buffer }
    }

    fn make_spi_read_request(address: usize) -> Vec<u16> {
        let address = (address & 0x3f) as u16;
        let mut bit_stream = vec![0; 2 + 6 + 1];

        // 2 bits "11" (Read Request)
        bit_stream[0] = 1;
        bit_stream[1] = 1;
        // 6 bits eeprom address
        bit_stream[2] = if address.bit(5) { 1 } else { 0 };
        bit_stream[3] = if address.bit(4) { 1 } else { 0 };
        bit_stream[4] = if address.bit(3) { 1 } else { 0 };
        bit_stream[5] = if address.bit(2) { 1 } else { 0 };
        bit_stream[6] = if address.bit(1) { 1 } else { 0 };
        bit_stream[7] = if address.bit(0) { 1 } else { 0 };
        // 1 stop bit
        bit_stream[8] = 0;

        bit_stream
    }

    fn make_spi_write_request(address: usize, value: [u8; 8]) -> Vec<u16> {
        let address = (address & 0x3f) as u16;
        let mut bit_stream = vec![0; 2 + 6 + 64 + 1];

        // 2 bits "10" (Write Request)
        bit_stream[0] = 1;
        bit_stream[1] = 0;

        // 6 bits eeprom address
        bit_stream[2] = if address.bit(5) { 1 } else { 0 };
        bit_stream[3] = if address.bit(4) { 1 } else { 0 };
        bit_stream[4] = if address.bit(3) { 1 } else { 0 };
        bit_stream[5] = if address.bit(2) { 1 } else { 0 };
        bit_stream[6] = if address.bit(1) { 1 } else { 0 };
        bit_stream[7] = if address.bit(0) { 1 } else { 0 };

        // encode the 64bit value
        for i in 0..8 {
            let mut byte = value[i];
            for j in 0..8 {
                let bit = byte >> 7;
                byte = byte << 1;
                bit_stream[8 + i * 8 + j] = bit as u16;
            }
        }

        // 1 stop bit
        bit_stream[2 + 6 + 64] = 0;

        bit_stream
    }

    #[test]
    fn test_spi_read() {
        let memory = make_mock_memory();
        let mut spi = SpiController::<MockMemory>::new(memory);

        // 1 bit "0" - stop bit
        let stream = make_spi_read_request(2);
        for half in stream.into_iter() {
            spi.write_half(half);
        }

        spi.consume_dummy_cycles();

        assert!(spi.chip.borrow().data_available());

        let data = spi.rx_data();

        assert_eq!(data[0], 'T' as u8);
        assert_eq!(data[1], 'E' as u8);
        assert_eq!(data[2], 'S' as u8);
        assert_eq!(data[3], 'T' as u8);
        assert_eq!(data[4], '!' as u8);

        assert_eq!(spi.chip.borrow().state, SpiState::RxInstruction);
        assert_eq!(spi.chip.borrow().rx_count, 0);
    }

    #[test]
    fn test_spi_read_write() {
        let memory = make_mock_memory();
        let mut spi = SpiController::<MockMemory>::new(memory);

        let expected = "Work.".as_bytes();

        // First, lets test a read request
        let stream = make_spi_read_request(2);
        for half in stream.into_iter() {
            spi.write_half(half);
        }
        spi.consume_dummy_cycles();
        let data = spi.rx_data();

        assert_eq!("TEST!".as_bytes(), &data[0..5]);
        {
            let chip = spi.chip.borrow();
            assert_eq!(SpiState::RxInstruction, chip.state);
            assert_eq!(0, chip.rx_count);
            assert_eq!(0, chip.rx_count);
        }

        // Now, modify the eeprom data with a write request
        let mut bytes: [u8; 8] = [0; 8];
        bytes[0] = expected[0];
        bytes[1] = expected[1];
        bytes[2] = expected[2];
        bytes[3] = expected[3];
        bytes[4] = expected[4];
        let stream = make_spi_write_request(2, bytes);
        for half in stream.into_iter() {
            spi.write_half(half);
        }

        {
            let chip = spi.chip.borrow();
            assert_eq!(expected, &chip.memory.buffer[0x10..0x15]);
            assert_eq!(SpiState::RxInstruction, chip.state);
            assert_eq!(0, chip.rx_count);
            assert_eq!(0, chip.tx_count);
        }

        // Also lets again read the result
        let stream = make_spi_read_request(2);
        for half in stream.into_iter() {
            spi.write_half(half);
        }
        spi.consume_dummy_cycles();
        let data = spi.rx_data();
        assert_eq!(expected, &data[0..5]);
    }
}
