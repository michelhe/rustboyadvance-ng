use super::{BackupFile, BackupMemoryInterface};

use bytesize;
use num::FromPrimitive;
use serde::{Deserialize, Serialize};

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum EepromType {
    Eeprom512,
    Eeprom8k,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
enum EepromAddressBits {
    Eeprom6bit,
    Eeprom14bit,
}

impl EepromType {
    fn size(&self) -> usize {
        match self {
            EepromType::Eeprom512 => 0x0200,
            EepromType::Eeprom8k => 0x2000,
        }
    }
    fn bits(&self) -> EepromAddressBits {
        match self {
            EepromType::Eeprom512 => EepromAddressBits::Eeprom6bit,
            EepromType::Eeprom8k => EepromAddressBits::Eeprom14bit,
        }
    }
}

impl Into<usize> for EepromAddressBits {
    fn into(self) -> usize {
        match self {
            EepromAddressBits::Eeprom6bit => 6,
            EepromAddressBits::Eeprom14bit => 14,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Primitive, PartialEq, Copy, Clone)]
enum SpiInstruction {
    Read = 0b011,
    Write = 0b010,
}

impl Default for SpiInstruction {
    fn default() -> SpiInstruction {
        SpiInstruction::Read /* TODO this is an arbitrary choice */
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
enum SpiState {
    RxInstruction,
    RxAddress(SpiInstruction),
    StopBit(SpiInstruction),
    TxDummy,
    TxData,
    RxData,
}

impl Default for SpiState {
    fn default() -> SpiState {
        SpiState::RxInstruction
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EepromChip {
    memory: BackupFile,
    addr_bits: EepromAddressBits,

    state: SpiState,
    rx_count: usize,
    rx_buffer: u64,

    tx_count: usize,
    tx_buffer: u64,

    address: usize,

    chip_ready: bool, // used to signal that the eeprom program was finished
                      // In real hardware, it takes some time for the values to be programmed into the eeprom,
                      // But we do it right away.
}

impl EepromChip {
    fn new(eeprom_type: EepromType, mut memory: BackupFile) -> EepromChip {
        memory.resize(eeprom_type.size());
        EepromChip {
            memory: memory,
            addr_bits: eeprom_type.bits(),

            state: SpiState::RxInstruction,

            rx_count: 0,
            rx_buffer: 0,

            tx_count: 0,
            tx_buffer: 0,

            address: 0,

            chip_ready: false,
        }
    }

    fn set_type(&mut self, eeprom_type: EepromType) {
        self.addr_bits = eeprom_type.bits();
        self.memory.resize(eeprom_type.size());
    }

    fn reset_rx_buffer(&mut self) {
        self.rx_buffer = 0;
        self.rx_count = 0;
    }

    fn reset_tx_buffer(&mut self) {
        self.tx_buffer = 0;
        self.tx_count = 0;
    }

    fn fill_tx_buffer(&mut self) {
        let mut tx_buffer = 0u64;
        for i in 0..8 {
            tx_buffer = tx_buffer << 8;
            tx_buffer |= self.memory.read(self.address + i) as u64;
        }
        self.tx_buffer = tx_buffer;
        self.tx_count = 0;
    }

    fn clock_data_in(&mut self, address: u32, si: u8) {
        use SpiInstruction::*;
        use SpiState::*;

        // Read the si signal into the rx_buffer
        trace!("({:?}) addr={:#x} RX bit {}", self.state, address, si);
        self.rx_buffer = (self.rx_buffer << 1) | (if si & 1 != 0 { 1 } else { 0 });
        self.rx_count += 1;

        let mut next_state: Option<SpiState> = None;

        match self.state {
            RxInstruction => {
                // If instruction was recvd, proceed to recv the address
                if self.rx_count >= 2 {
                    let insn = SpiInstruction::from_u64(self.rx_buffer).expect(&format!(
                        "invalid spi command {:#010b}",
                        self.rx_buffer as u8
                    ));
                    next_state = Some(RxAddress(insn));
                    self.reset_rx_buffer();
                }
            }
            RxAddress(insn) => {
                if self.rx_count == self.addr_bits.into() {
                    self.address = (self.rx_buffer as usize) * 8;
                    trace!(
                        "{:?} mode , recvd address = {:#x} (rx_buffer={:#x})",
                        insn,
                        self.address,
                        self.rx_buffer
                    );
                    match insn {
                        Read => {
                            next_state = Some(StopBit(Read));
                        }
                        Write => {
                            next_state = Some(RxData);
                            self.chip_ready = false;
                            self.reset_rx_buffer();
                        }
                    }
                }
            }
            StopBit(Read) => {
                next_state = Some(TxDummy);
                self.reset_rx_buffer();
                self.reset_tx_buffer();
            }
            RxData => {
                if self.rx_count == 64 {
                    let mut data = self.rx_buffer;
                    debug!("writing {:#x} to memory address {:#x}", data, self.address);
                    for i in 0..8 {
                        self.memory
                            .write(self.address + (7 - i), (data & 0xff) as u8);
                        data = data >> 8;
                    }
                    next_state = Some(StopBit(Write));
                    self.reset_rx_buffer();
                }
            }
            StopBit(Write) => {
                self.chip_ready = true;
                self.state = RxInstruction;
                self.reset_rx_buffer();
                self.reset_tx_buffer();
            }
            _ => {}
        }
        if let Some(next_state) = next_state {
            self.state = next_state;
        }
    }

    fn clock_data_out(&mut self, address: u32) -> u8 {
        use SpiState::*;

        let mut next_state = None;
        let result = match self.state {
            TxDummy => {
                self.tx_count += 1;
                if self.tx_count == 4 {
                    next_state = Some(TxData);
                    self.fill_tx_buffer();
                    trace!("transmitting data bits, tx_buffer = {:#x}", self.tx_buffer);
                }
                0
            }
            TxData => {
                let result = ((self.tx_buffer >> 63) & 1) as u8;
                self.tx_buffer = self.tx_buffer.wrapping_shl(1);
                self.tx_count += 1;
                if self.tx_count == 64 {
                    self.reset_tx_buffer();
                    self.reset_rx_buffer();
                    next_state = Some(RxInstruction);
                }
                result
            }
            _ => {
                if self.chip_ready {
                    1
                } else {
                    0
                }
            }
        };

        trace!("({:?}) addr={:#x} TX bit {}", self.state, address, result);
        if let Some(next_state) = next_state {
            self.state = next_state;
        }

        result
    }

    pub(in crate) fn is_transmitting(&self) -> bool {
        use SpiState::*;
        match self.state {
            TxData | TxDummy => true,
            _ => false,
        }
    }

    pub(in crate) fn reset(&mut self) {
        self.state = SpiState::RxInstruction;
        self.reset_rx_buffer();
        self.reset_tx_buffer();
    }
}

// #[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone, Default)]
// struct ChipSizeDetectionState {
//     insn: SpiInstruction,
//     rx_buffer: u64,
//     rx_count: usize,
// }

/// The Eeprom controller is usually mapped to the top 256 bytes of the cartridge memory
/// Eeprom controller can programmed with DMA accesses in 16bit mode
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EepromController {
    pub(in crate) chip: RefCell<EepromChip>,
    detect: bool,
}

impl EepromController {
    pub fn new(path: Option<PathBuf>) -> EepromController {
        let mut detect = true;
        let mut eeprom_type = EepromType::Eeprom512;
        if let Some(path) = &path {
            if let Ok(metadata) = fs::metadata(&path) {
                let human_size = bytesize::ByteSize::b(metadata.len());
                let assumed_type = match metadata.len() {
                    512 => EepromType::Eeprom512,
                    8192 => EepromType::Eeprom8k,
                    _ => panic!("invalid file size ({}) for eeprom save", human_size),
                };
                detect = false;
                info!(
                    "save file is size {}, assuming eeprom type is {:?}",
                    human_size, assumed_type
                );
                eeprom_type = assumed_type;
            }
        }

        let mut result = EepromController::new_with_type(path, eeprom_type);
        result.detect = detect;

        result
    }

    pub fn new_with_type(path: Option<PathBuf>, eeprom_type: EepromType) -> EepromController {
        let memory = BackupFile::new(eeprom_type.size(), path);
        EepromController {
            chip: RefCell::new(EepromChip::new(eeprom_type, memory)),
            detect: false,
        }
    }

    pub fn write_half(&mut self, address: u32, value: u16) {
        assert!(!self.detect);
        self.chip.borrow_mut().clock_data_in(address, value as u8);
    }

    pub fn read_half(&self, address: u32) -> u16 {
        assert!(!self.detect);
        let mut chip = self.chip.borrow_mut();
        chip.clock_data_out(address) as u16
    }

    pub fn on_dma3_transfer(&mut self, src: u32, dst: u32, count: usize) {
        use EepromType::*;
        if self.detect {
            match (src, dst) {
                // DMA to EEPROM
                (_, 0x0d000000..=0x0dffffff) => {
                    debug!(
                        "caught eeprom dma transfer src={:#x} dst={:#x} count={}",
                        src, dst, count
                    );
                    let eeprom_type = match count {
                        // Read(11) + 6bit address + stop bit
                        9 => Eeprom512,
                        // Read(11) + 14bit address + stop bit
                        17 => Eeprom8k,
                        // Write(11) + 6bit address + 64bit value + stop bit
                        73 => Eeprom512,
                        // Write(11) + 14bit address + 64bit value + stop bit
                        81 => Eeprom8k,
                        _ => panic!(
                            "unexpected bit count ({}) when detecting eeprom size",
                            count
                        ),
                    };
                    info!("detected eeprom type: {:?}", eeprom_type);
                    self.chip.borrow_mut().set_type(eeprom_type);
                    self.detect = false;
                }
                // EEPROM to DMA
                (0x0d000000..=0x0dffffff, _) => {
                    panic!("reading from eeprom when real size is not detected yet is not supported by this emulator")
                }
                _ => { /* Not a eeprom dma, doing nothing */ }
            }
        } else {
            // this might be a eeprom request, so we need to reset the eeprom state machine if its dirty (due to bad behaving games, or tests roms)
            let mut chip = self.chip.borrow_mut();
            if !chip.is_transmitting() {
                chip.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::EEPROM_BASE_ADDR;
    use super::*;

    use bit::BitIndex;

    impl EepromController {
        fn consume_dummy_cycles(&self) {
            // ignore the dummy bits
            self.read_half(EEPROM_BASE_ADDR);
            self.read_half(EEPROM_BASE_ADDR);
            self.read_half(EEPROM_BASE_ADDR);
            self.read_half(EEPROM_BASE_ADDR);
        }

        fn rx_data(&self) -> [u8; 8] {
            let mut bytes = [0; 8];
            for byte_index in 0..8 {
                let mut byte = 0u8;
                for _ in 0..8 {
                    let bit = self.read_half(EEPROM_BASE_ADDR) as u8;
                    byte = (byte.wrapping_shl(1)) | bit;
                }
                bytes[byte_index] = byte;
            }
            bytes
        }
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
    fn test_spi_read_write() {
        let mut spi = EepromController::new_with_type(None, EepromType::Eeprom512);
        // hacky way to initialize the backup file with contents.
        // TODO - implement EepromController initialization with data buffer and not files
        {
            let mut chip = spi.chip.borrow_mut();
            let bytes = chip.memory.bytes_mut();
            bytes[16] = 'T' as u8;
            bytes[17] = 'E' as u8;
            bytes[18] = 'S' as u8;
            bytes[19] = 'T' as u8;
            bytes[20] = '!' as u8;
            drop(bytes);
            chip.memory.flush();
        }

        let expected = "Work.".as_bytes();

        // First, lets test a read request
        let stream = make_spi_read_request(2);
        for half in stream.into_iter() {
            spi.write_half(EEPROM_BASE_ADDR, half);
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
            spi.write_half(EEPROM_BASE_ADDR, half);
        }

        {
            let chip = spi.chip.borrow();
            assert_eq!(expected, &chip.memory.bytes()[0x10..0x15]);
            assert_eq!(SpiState::RxInstruction, chip.state);
            assert_eq!(0, chip.rx_count);
            assert_eq!(0, chip.tx_count);
        }

        // Also lets again read the result
        let stream = make_spi_read_request(2);
        for half in stream.into_iter() {
            spi.write_half(EEPROM_BASE_ADDR, half);
        }
        spi.consume_dummy_cycles();
        let data = spi.rx_data();
        assert_eq!(expected, &data[0..5]);
        {
            let chip = spi.chip.borrow();
            assert_eq!(SpiState::RxInstruction, chip.state);
            assert_eq!(0, chip.rx_count);
            assert_eq!(0, chip.tx_count);
        }
    }
}
