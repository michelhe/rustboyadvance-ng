use pyo3::prelude::*;
use pyo3::types::PyDict;
use rustboyadvance_core::prelude::*;
use std::path::Path;
use flexi_logger::{Logger, Duplicate};

use rustboyadvance_core::cartridge::loader::{load_from_file, LoadRom};

#[macro_use]
extern crate log;

#[pymodule]

fn rustboyadvance_py(_py: Python, m: &PyModule) -> PyResult<()> {
    // Initialize flexi_logger
    Logger::with_env_or_str("info") // Default log level is "info"
        .log_to_file() 
        .directory(".logs")      
        .duplicate_to_stderr(Duplicate::Debug) 
        .start()
        .unwrap();

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

        // Optional: warn if ELF and feature is not enabled
        #[cfg(not(feature = "elf_support"))]
        if rom_path.ends_with(".elf") {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "ELF ROM loading requested but rustboyadvance-ng was built without 'elf_support' feature.",
            ));
        }

        let builder = GamepakBuilder::new().file(Path::new(rom_path));
        let builder = builder
            .build()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Failed to load ROM: {e}")))?;
        let audio = NullAudio::new();

        self.core = Some(GameBoyAdvance::new(bios, builder, audio));
        Ok(())
    }

     /// Get the stop ID by name. Returns the ID or None if not found.
    pub fn get_stop_id(&self, name: String) -> PyResult<Option<u32>> {
        if let Some(core) = &self.core {
            Ok(core.get_stop_id(&name))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    fn add_stop_addr(&mut self, addr:u32, value:i16, is_active:bool , name:String, id:u32) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            debug!("Adding stop address: addr={}, value={}, is_active={}, name={}, id={}\n", &addr, &value, &is_active, &name,id);
            core.add_stop_addr(addr, value, is_active, name, id);
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

    pub fn run_to_next_stop(&mut self, cycles_to_run: usize) -> PyResult<i32> {
        match &mut self.core {
            Some(core) => Ok(core.run_to_next_stop(cycles_to_run)),
            None => Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded")),
        }
    }

    /// Get a stop address by name. Returns a dict or None.
    pub fn get_stop_addr(&self, name: String) -> PyResult<Option<PyObject>> {
        Python::with_gil(|py| {
            if let Some(core) = &self.core {
                if let Some(stop_addr) = core.get_stop_addr(name) {
                    let dict = PyDict::new(py);
                    dict.set_item("addr", stop_addr.addr)?;
                    dict.set_item("is_active", stop_addr.is_active)?;
                    dict.set_item("value", stop_addr.value)?;
                    dict.set_item("name", stop_addr.name.clone())?;
                    Ok(Some(dict.into()))
                } else {
                    Ok(None)
                }
            } else {
                Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
            }
        })
    }

    /// Check if a given address contains a value.
    pub fn check_addr(&self, addr: u32, value: i16) -> PyResult<bool> {
        if let Some(core) = &self.core {
            Ok(core.check_addr(addr, value))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Return a list of active stop addresses that match their value.
    pub fn check_stop_addrs(&self) -> PyResult<Vec<PyObject>> {
        Python::with_gil(|py| {
            if let Some(core) = &self.core {
                let result: Vec<PyObject> = core
                    .check_stop_addrs()
                    .into_iter()
                    .map(|stop_addr| {
                        let dict = PyDict::new(py);
                        dict.set_item("addr", stop_addr.addr).unwrap();
                        dict.set_item("is_active", stop_addr.is_active).unwrap();
                        dict.set_item("value", stop_addr.value).unwrap();
                        dict.set_item("name", stop_addr.name.clone()).unwrap();
                        dict.into()
                    })
                    .collect();
                Ok(result)
            } else {
                Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
            }
        })
    }

    /// Read a u16 from EWRAM.
    pub fn read_u16(&self, addr: u32) -> PyResult<u16> {
        if let Some(core) = &self.core {
            Ok(core.read_u16(addr))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Write a u16 to EWRAM.
    pub fn write_u16(&mut self, addr: u32, value: u16) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.write_u16(addr, value);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Read a u32 from EWRAM.
    pub fn read_u32(&self, addr: u32) -> PyResult<u32> {
        if let Some(core) = &self.core {
            Ok(core.read_u32(addr))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Write a u32 to EWRAM.
    pub fn write_u32(&mut self, addr: u32, value: u32) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.write_u32(addr, value);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Read a list of u16 from EWRAM.
    pub fn read_u16_list(&self, addr: u32, count: usize) -> PyResult<Vec<u16>> {
        if let Some(core) = &self.core {
            Ok(core.read_u16_list(addr, count))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    pub fn get_frame_buffer(&self) -> PyResult<Vec<u32>> {
        if let Some(core) = &self.core {
            Ok(core.get_frame_buffer().to_vec())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    pub fn write_u32_list(&mut self, addr: u32, values: Vec<u32>) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.write_u32_list(addr, &values);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Read a single i8 from EWRAM.
    pub fn read_i8(&self, addr: u32) -> PyResult<i8> {
        if let Some(core) = &self.core {
            Ok(core.read_i8(addr))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Write a single i8 to EWRAM.
    pub fn write_i8(&mut self, addr: u32, value: i8) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.write_i8(addr, value);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Read a list of i8 from EWRAM.
    pub fn read_i8_list(&self, addr: u32, count: usize) -> PyResult<Vec<i8>> {
        if let Some(core) = &self.core {
            Ok(core.read_i8_list(addr, count))
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Write a list of i8 to EWRAM.
    pub fn write_i8_list(&mut self, addr: u32, values: Vec<i8>) -> PyResult<()> {
        if let Some(core) = &mut self.core {
            core.write_i8_list(addr, &values);
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }

    /// Load a savestate from a file
    pub fn load_savestate(
        &mut self,
        savestate_path: &str,
        bios_path: &str,
        rom_path: &str,
    ) -> PyResult<()> {
        let savestate_file = Path::new(savestate_path);
        if !savestate_file.is_file() {
            return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
                "Savestate file not found: {}",
                savestate_path
            )));
        }

        let save = std::fs::read(savestate_file).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to read savestate: {}", e))
        })?;

        let bios = load_bios(Path::new(bios_path));

        let rom_path = Path::new(rom_path);
        let rom = match load_from_file(rom_path).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to load ROM: {}", e))
        })? {
            LoadRom::Raw(data) => data.into_boxed_slice(),
            LoadRom::Elf { data, .. } => data.into_boxed_slice(),
        };

        let audio = NullAudio::new();

        self.core = Some(
            GameBoyAdvance::from_saved_state(&save, bios, rom, audio).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Failed to restore savestate: {}",
                    e
                ))
            })?,
        );

        Ok(())
    }

    pub fn save_savestate(&self, savestate_path: &str) -> PyResult<()> {
        if let Some(core) = &self.core {
            let savestate_file = Path::new(savestate_path);
            let save_data = core.save_state().map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to save state: {}", e))
            })?;

            std::fs::write(savestate_file, save_data).map_err(|e| {
                pyo3::exceptions::PyIOError::new_err(format!("Failed to write savestate: {}", e))
            })?;
            Ok(())
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("GBA core not loaded"))
        }
    }


}
