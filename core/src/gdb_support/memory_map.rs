use xml_builder::{XMLBuilder, XMLElement, XMLVersion};

use arm7tdmi::{
    gdb::{copy_range_to_buf, target::MemoryGdbInterface},
    memory::Addr,
};

use crate::sysbus::{consts, SysBus};

impl SysBus {
    pub fn generate_memory_map_xml(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut xml = XMLBuilder::new()
            .version(XMLVersion::XML1_1)
            .encoding("UTF-8".into())
            .build();
        let mut memory_map = XMLElement::new("memory-map");

        let mut add_memory = |start: Addr, length: usize| -> Result<(), String> {
            let mut memory = XMLElement::new("memory");
            memory.add_attribute("type", "ram"); // using "ram" for everything to allow use of sw-breakpoints
            memory.add_attribute("start", &start.to_string());
            memory.add_attribute("length", &length.to_string());
            memory_map
                .add_child(memory)
                .map_err(|e| format!("failed to add child: {:?}", e))?;
            Ok(())
        };

        add_memory(consts::BIOS_ADDR, self.bios.len())?;
        add_memory(consts::EWRAM_ADDR, self.ewram.len())?;
        add_memory(consts::IWRAM_ADDR, self.iwram.len())?;
        add_memory(consts::IOMEM_ADDR, 0x400)?;
        add_memory(consts::PALRAM_ADDR, self.io.gpu.palette_ram.len())?;
        add_memory(consts::VRAM_ADDR, self.io.gpu.vram.len())?;
        add_memory(consts::OAM_ADDR, self.io.gpu.oam.len())?;
        add_memory(consts::CART_BASE, self.cartridge.get_rom_bytes().len())?;

        xml.set_root_element(memory_map);
        let mut writer = Vec::new();
        xml.generate(&mut writer)
            .map_err(|e| format!("failed to generate xml: {:?}", e))?;

        Ok(String::from_utf8(writer)?)
    }
}

impl MemoryGdbInterface for SysBus {
    fn memory_map_xml(&self, offset: u64, length: usize, buf: &mut [u8]) -> usize {
        copy_range_to_buf(
            self.generate_memory_map_xml().unwrap().as_bytes(),
            offset,
            length,
            buf,
        )
    }
}
