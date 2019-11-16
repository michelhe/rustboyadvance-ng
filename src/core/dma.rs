use std::collections::VecDeque;

use super::arm7tdmi::{Addr, Bus};
use super::sysbus::SysBus;
use super::{Interrupt, IrqBitmask, SyncedIoDevice};

use num::FromPrimitive;

#[derive(Debug)]
enum DmaTransferType {
    Xfer16bit,
    Xfer32bit,
}

#[derive(Debug)]
pub struct DmaChannel {
    id: usize,

    pub src: u32,
    pub dst: u32,
    pub wc: u32,
    pub ctrl: DmaChannelCtrl,

    // These are "latched" when the dma is enabled.
    internal: DmaInternalRegs,

    running: bool,
    cycles: usize,
    start_cycles: usize,
    irq: Interrupt,
}

#[derive(Debug, Default)]
struct DmaInternalRegs {
    src_addr: u32,
    dst_addr: u32,
    count: u32,
}

impl DmaChannel {
    pub fn new(id: usize) -> DmaChannel {
        if id > 3 {
            panic!("invalid dma id {}", id);
        }
        DmaChannel {
            id: id,
            irq: Interrupt::from_usize(id + 8).unwrap(),
            running: false,
            src: 0,
            dst: 0,
            wc: 0,
            ctrl: DmaChannelCtrl(0),
            cycles: 0,
            start_cycles: 0,
            internal: Default::default(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn write_src_low(&mut self, low: u16) {
        let src = self.src;
        self.src = (src & 0xffff0000) | (low as u32);
    }

    pub fn write_src_high(&mut self, high: u16) {
        let src = self.src;
        let high = high as u32;
        self.src = (src & 0xffff) | (high << 16);
    }

    pub fn write_dst_low(&mut self, low: u16) {
        let dst = self.dst;
        self.dst = (dst & 0xffff0000) | (low as u32);
    }

    pub fn write_dst_high(&mut self, high: u16) {
        let dst = self.dst;
        let high = high as u32;
        self.dst = (dst & 0xffff) | (high << 16);
    }

    pub fn write_word_count(&mut self, value: u16) {
        self.wc = value as u32;
    }

    pub fn write_dma_ctrl(&mut self, value: u16) -> bool {
        let ctrl = DmaChannelCtrl(value);
        let mut start_immediately = false;
        if ctrl.is_enabled() && !self.ctrl.is_enabled() {
            self.start_cycles = self.cycles;
            self.running = true;
            start_immediately = ctrl.timing() == 0;
            self.internal.src_addr = self.src;
            self.internal.dst_addr = self.dst;
            self.internal.count = self.wc;
        }
        self.ctrl = ctrl;
        return start_immediately;
    }

    fn xfer(&mut self, sb: &mut SysBus, irqs: &mut IrqBitmask) {
        let word_size = if self.ctrl.is_32bit() { 4 } else { 2 };
        let count = match self.internal.count {
            0 => match self.id {
                3 => 0x1_0000,
                _ => 0x0_4000,
            },
            _ => self.internal.count,
        };
        for _ in 0..count {
            if word_size == 4 {
                let w = sb.read_32(self.internal.src_addr);
                sb.write_32(self.internal.dst_addr, w)
            } else {
                let hw = sb.read_16(self.internal.src_addr);
                sb.write_16(self.internal.dst_addr, hw)
            }
            match self.ctrl.src_adj() {
                /* Increment */ 0 => self.internal.src_addr += word_size,
                /* Decrement */ 1 => self.internal.src_addr -= word_size,
                /* Fixed */ 2 => {}
                _ => panic!("forbidden DMA source address adjustment"),
            }
            match self.ctrl.dst_adj() {
                /* Increment[+Reload] */ 0 | 3 => self.internal.dst_addr += word_size,
                /* Decrement */ 1 => self.internal.dst_addr -= word_size,
                /* Fixed */ 2 => {}
                _ => panic!("forbidden DMA dest address adjustment"),
            }
        }
        if self.ctrl.is_triggering_irq() {
            irqs.add_irq(self.irq);
        }
        if self.ctrl.repeat() {
            self.start_cycles = self.cycles;
            /* reload */
            if 3 == self.ctrl.dst_adj() {
                self.internal.dst_addr = self.dst;
            }
        } else {
            self.running = false;
            self.ctrl.set_enabled(false);
        }
    }
}

#[derive(Debug)]
pub struct DmaController {
    pub channels: [DmaChannel; 4],
    xfers_queue: VecDeque<usize>,
    cycles: usize,
}

impl DmaController {
    pub fn new() -> DmaController {
        DmaController {
            channels: [
                DmaChannel::new(0),
                DmaChannel::new(1),
                DmaChannel::new(2),
                DmaChannel::new(3),
            ],
            xfers_queue: VecDeque::new(),
            cycles: 0,
        }
    }

    pub fn perform_work(&mut self, sb: &mut SysBus, irqs: &mut IrqBitmask) -> bool {
        if self.xfers_queue.is_empty() {
            false
        } else {
            while let Some(id) = self.xfers_queue.pop_front() {
                self.channels[id].xfer(sb, irqs)
            }
            true
        }
    }

    pub fn write_16(&mut self, channel_id: usize, ofs: u32, value: u16) {
        match ofs {
            0 => self.channels[channel_id].write_src_low(value),
            2 => self.channels[channel_id].write_src_high(value),
            4 => self.channels[channel_id].write_dst_low(value),
            6 => self.channels[channel_id].write_dst_high(value),
            8 => self.channels[channel_id].write_word_count(value),
            10 => {
                if self.channels[channel_id].write_dma_ctrl(value) {
                    self.xfers_queue.push_back(channel_id)
                }
            }
            _ => panic!("Invalid dma offset"),
        }
    }

    pub fn notify_vblank(&mut self) {
        for i in 0..4 {
            if self.channels[i].ctrl.is_enabled() && self.channels[i].ctrl.timing() == 1 {
                self.xfers_queue.push_back(i);
            }
        }
    }

    pub fn notify_hblank(&mut self) {
        for i in 0..4 {
            if self.channels[i].ctrl.is_enabled() && self.channels[i].ctrl.timing() == 2 {
                self.xfers_queue.push_back(i);
            }
        }
    }
}

bitfield! {
    #[derive(Default)]
    pub struct DmaChannelCtrl(u16);
    impl Debug;
    u16;
    dst_adj, _ : 6, 5;
    src_adj, _ : 8, 7;
    repeat, _ : 9;
    is_32bit, _: 10;
    timing, _: 13, 12;
    is_triggering_irq, _: 14;
    is_enabled, set_enabled: 15;
}
