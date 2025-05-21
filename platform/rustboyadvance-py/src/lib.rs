use pyo3::prelude::*;
use rustboyadvance_core::prelude::*;
use std::fs::read;

#[pymodule]
fn rustboyadvance_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<RustGba>()?;
    Ok(())
}

#[pyclass(unsendable)]
pub struct RustGba {
    core: Option<GameBoyAdvance>,
}

#[pymethods]
impl RustGba {
    #[new]
    pub fn new() -> Self {
        RustGba { core: None }
    }

    pub fn load(&mut self, bios_path: &str, rom_path: &str) -> PyResult<()> {
        let bios = read(bios_path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(format!("Failed to read BIOS: {e}")))?;
        let rom = read(rom_path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(format!("Failed to read ROM: {e}")))?;

        let bios = bios.into_boxed_slice();
        let cartridge = GamepakBuilder::new().buffer(&rom).build().unwrap();
        let audio = NullAudio::new();

        self.core = Some(GameBoyAdvance::new(bios, cartridge, audio));
        Ok(())
    }
}