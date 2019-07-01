use std::fs::File;
use std::io;
use std::io::prelude::*;

pub fn read_bin_file(filename: &str) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut buf)?;
    Ok(buf)
}
