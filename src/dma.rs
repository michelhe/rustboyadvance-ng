use super::arm7tdmi::{Addr, Bus};
use super::ioregs::consts::*;
use super::sysbus::SysBus;
use super::{EmuIoDev, Interrupt};

#[allow(non_camel_case_types)]
#[derive(Debug)]
pub struct DmaChannel {
    src_ioreg: Addr, /* Source Address register */
    dst_ioreg: Addr, /* Destination Address register */
    wc_ioreg: Addr,  /* Word Count 14bit */
}

#[derive(Debug, Primitive)]
enum DmaAddrControl {
    Increment = 0,
    Decrement = 1,
    Fixed = 2,
    IncrementReloadProhibited = 3,
}

#[derive(Debug)]
enum DmaTransferType {
    Xfer16bit,
    Xfer32bit,
}

#[derive(Debug, Primitive)]
enum DmaStartTiming {
    Immediately = 0,
    VBlank = 1,
    HBlank = 2,
    Special = 3,
}

#[derive(Debug)]
struct DmaControl {
    dst_addr_ctl: DmaAddrControl,
    src_addr_ctl: DmaAddrControl,
    repeat: bool,
    xfer: DmaTransferType,
    start_timing: DmaStartTiming,
    irq_upon_end_of_wc: bool,
    enable: bool,
}

impl DmaChannel {
    pub fn new(src_ioreg: Addr, dst_ioreg: Addr, wc_ioreg: Addr) -> DmaChannel {
        DmaChannel {
            src_ioreg,
            dst_ioreg,
            wc_ioreg,
        }
    }

    fn src_addr(&self, sysbus: &SysBus) -> Addr {
        sysbus.ioregs.read_32(self.src_ioreg - IO_BASE) as Addr
    }

    fn dst_addr(&self, sysbus: &SysBus) -> Addr {
        sysbus.ioregs.read_32(self.dst_ioreg - IO_BASE) as Addr
    }

    fn word_count(&self, sysbus: &SysBus) -> usize {
        sysbus.ioregs.read_reg(self.wc_ioreg) as usize
    }
}

impl EmuIoDev for DmaChannel {
    fn step(&mut self, cycles: usize, sysbus: &mut SysBus) -> (usize, Option<Interrupt>) {
        // TODO
        (0, None)
    }
}
