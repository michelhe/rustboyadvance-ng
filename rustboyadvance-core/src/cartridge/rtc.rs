use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
enum Port {
    #[doc("Serial Clock")]
    Sck = 0,
    #[doc("Serial IO")]
    Sio = 1,
    #[doc("Chip Select")]
    Cs = 2,
}

/// Model of the S3511 8pin RTC
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rtc {}

impl Rtc {
    pub fn new() -> Self {
        Rtc {}
    }

    pub fn read_port(port: usize) -> u8 {
        0
    }

    pub fn write_port(port: usize) {}
}
