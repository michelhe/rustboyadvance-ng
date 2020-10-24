/// mGBA 0.8.1 Debug peripheral support
use std::str;

use log::log;
use log::Level;

use super::bus::Bus;
use super::iodev::consts::{REG_DEBUG_ENABLE, REG_DEBUG_FLAGS, REG_DEBUG_STRING};

pub const DEBUG_STRING_SIZE: usize = 0x100;

#[derive(Clone, Serialize, Deserialize)]
pub struct DebugPort {
    enable: bool,
    flags: DebugFlags,
    debug_string: Box<[u8]>,
}

impl DebugPort {
    pub fn new() -> DebugPort {
        DebugPort {
            enable: false,
            flags: DebugFlags(0),
            debug_string: vec![0; DEBUG_STRING_SIZE].into_boxed_slice(),
        }
    }

    #[inline]
    pub fn is_debug_access(x: u32) -> bool {
        x == REG_DEBUG_ENABLE
            || x == REG_DEBUG_FLAGS
            || (x >= REG_DEBUG_STRING && x <= REG_DEBUG_STRING + (DEBUG_STRING_SIZE as u32))
    }

    pub fn read(&mut self, addr: u32) -> u16 {
        if !self.enable {
            return 0;
        }
        match addr {
            REG_DEBUG_ENABLE => 0x1DEA,
            REG_DEBUG_FLAGS => self.flags.0,
            x if x >= REG_DEBUG_STRING && x <= REG_DEBUG_STRING + (DEBUG_STRING_SIZE as u32) => {
                self.debug_string.read_16(addr - REG_DEBUG_STRING)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u32, value: u16) {
        match addr {
            REG_DEBUG_ENABLE => {
                self.enable = value == 0xC0DE;
                info!("mGBA log enabled: {}", self.enable);
            }
            REG_DEBUG_FLAGS => {
                if self.enable {
                    self.flags.0 = value;
                    self.debug();
                }
            }
            x if x >= REG_DEBUG_STRING && x <= REG_DEBUG_STRING + (DEBUG_STRING_SIZE as u32) => {
                if self.enable {
                    self.debug_string.write_16(addr - REG_DEBUG_STRING, value);
                }
            }
            _ => unreachable!(),
        }
    }

    fn debug(&mut self) {
        if self.flags.send() {
            let message = str::from_utf8(&self.debug_string)
                .expect("Failed to parse log message to valid utf8");

            let level: Level = match self.flags.level() {
                0 | 1 => Level::Error,
                2 => Level::Warn,
                3 => Level::Info,
                4 => Level::Debug,
                _ => panic!("invalid log level"),
            };

            log!(level, "[mGBA mLOG]: {}", message);

            for i in self.debug_string.iter_mut() {
                *i = 0;
            }

            self.flags.set_send(false);
        }
    }
}

bitfield! {
    #[derive(Clone, Serialize, Deserialize)]
    pub struct DebugFlags(u16);
    impl Debug;
    u16;
    pub into usize, level, _: 3, 0;
    pub send, set_send: 8;
}
