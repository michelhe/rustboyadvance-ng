use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::time;

pub fn read_bin_file(filename: &Path) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub struct FpsCounter {
    count: u32,
    timer: time::Instant,
}

impl Default for FpsCounter {
    fn default() -> FpsCounter {
        FpsCounter {
            count: 0,
            timer: time::Instant::now(),
        }
    }
}

impl FpsCounter {
    pub fn tick(&mut self) -> Option<u32> {
        self.count += 1;
        if self.timer.elapsed() >= time::Duration::from_secs(1) {
            let fps = self.count;
            self.timer = time::Instant::now();
            self.count = 0;
            Some(fps)
        } else {
            None
        }
    }
}

#[macro_export]
macro_rules! index2d {
    ($x:expr, $y:expr, $w:expr) => {
        $w * $y + $x
    };
    ($t:ty, $x:expr, $y:expr, $w:expr) => {
        (($w as $t) * ($y as $t) + ($x as $t)) as $t
    };
}

macro_rules! host_breakpoint {
    () => {
        #[cfg(debug_assertions)]
        unsafe {
            ::std::intrinsics::breakpoint()
        }
    };
}
