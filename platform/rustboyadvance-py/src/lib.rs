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

    pub fn run_cycles(&mut self, cycles: usize) -> PyResult<usize> {
        match &mut self.core {
            Some(core) => Ok(core.run::<false>(cycles)),
            None => Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded")),
        }
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

    fn add_stop_addr(&mut self, addr:u32, value:i16, is_active:bool , name:String) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.add_stop_addr(addr, value, is_active, name);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    fn remove_stop_addr(&mut self, addr: u32) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.remove_stop_addr(addr);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }
    //TODO: add test in example.py 
    pub fn read_u32_list(&self, addr: u32, count: usize) -> PyResult<Vec<u32>> {
        if let Some(core) = &self.core {
            Ok(core.read_u32_list(addr, count))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }


}