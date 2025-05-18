use pyo3::prelude::*;
use rustboyadvance_core::prelude::*;

#[pyclass(unsendable)]
pub struct RustGba {
}

#[pymethods]
impl RustGba {
    #[new]
    pub fn new() -> Self {
        
    }
}