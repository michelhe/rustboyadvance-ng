use pyo3::prelude::*;
use rustboyadvance_core::prelude::*;
use std::fs::read;
use std::path::Path;

#[pymodule]
fn rustboyadvance_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<RustGba>()?;
    Ok(())
}

#[pyclass(unsendable)]
pub struct RustGba {
    core: Option<GameBoyAdvance>,
}

fn load_bios(bios_path: &Path) -> Box<[u8]> {
    match read_bin_file(bios_path) {
        Ok(bios) => bios.into_boxed_slice(),
        _ => {
            // You can print a message or raise a Python exception here
            panic!("Missing BIOS file: {:?}", bios_path);
        }
    }
}

#[pymethods]
impl RustGba {
    #[new]
    pub fn new() -> Self {
        RustGba { core: None }
    }

    pub fn load(&mut self, bios_path: &str, rom_path: &str) -> PyResult<()> {
       
        let bios = load_bios(Path::new(bios_path));

        let builder = GamepakBuilder::new().file(Path::new(rom_path));
        
        let cartridge = builder
            .build()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Failed to load ROM: {e}")))?;
        let audio = NullAudio::new();

        self.core = Some(GameBoyAdvance::new(bios, cartridge, audio));
        Ok(())
    }
}