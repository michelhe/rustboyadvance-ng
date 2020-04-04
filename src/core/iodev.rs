use super::dma::DmaController;
use super::gpu::regs::WindowFlags;
use super::gpu::*;
use super::interrupt::InterruptController;
use super::keypad;
use super::sound::SoundController;
use super::sysbus::SysBusPtr;
use super::timer::Timers;
use super::{Addr, Bus};

use serde::{Deserialize, Serialize};

use self::consts::*;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum HaltState {
    Running,
    Halt, // In Halt mode, the CPU is paused as long as (IE AND IF)=0,
    Stop, // In Stop mode, most of the hardware including sound and video are paused
}

#[derive(Clone, Serialize, Deserialize)]
pub struct IoDevices {
    pub intc: InterruptController,
    pub gpu: Box<Gpu>,
    pub sound: Box<SoundController>,
    pub timers: Timers,
    pub dmac: DmaController,
    pub keyinput: u16,
    pub post_boot_flag: bool,
    pub waitcnt: WaitControl, // TODO also implement 4000800
    pub haltcnt: HaltState,

    // HACK
    // my ownership design sucks
    #[serde(skip)]
    #[serde(default = "SysBusPtr::default")]
    sysbus_ptr: SysBusPtr,
}

impl IoDevices {
    pub fn new(gpu: Box<Gpu>, sound_controller: Box<SoundController>) -> IoDevices {
        IoDevices {
            gpu: gpu,
            sound: sound_controller,
            timers: Timers::new(),
            dmac: DmaController::new(),
            intc: InterruptController::new(),
            post_boot_flag: false,
            haltcnt: HaltState::Running,
            keyinput: keypad::KEYINPUT_ALL_RELEASED,
            waitcnt: WaitControl(0),

            sysbus_ptr: Default::default(),
        }
    }

    pub fn set_sysbus_ptr(&mut self, ptr: SysBusPtr) {
        self.sysbus_ptr = ptr;
    }
}

impl Bus for IoDevices {
    fn read_16(&self, addr: Addr) -> u16 {
        let io = self;
        let io_addr = addr + IO_BASE;
        if addr > 0x0800 {
            return 0;
        }
        match io_addr {
            REG_DISPCNT => io.gpu.dispcnt.0,
            REG_DISPSTAT => io.gpu.dispstat.0,
            REG_VCOUNT => io.gpu.vcount as u16,
            REG_BG0CNT => io.gpu.backgrounds[0].bgcnt.0,
            REG_BG1CNT => io.gpu.backgrounds[1].bgcnt.0,
            REG_BG2CNT => io.gpu.backgrounds[2].bgcnt.0,
            REG_BG3CNT => io.gpu.backgrounds[3].bgcnt.0,
            REG_WIN0H => ((io.gpu.win0.left as u16) << 8 | (io.gpu.win0.right as u16)),
            REG_WIN1H => ((io.gpu.win1.left as u16) << 8 | (io.gpu.win1.right as u16)),
            REG_WIN0V => ((io.gpu.win0.top as u16) << 8 | (io.gpu.win0.bottom as u16)),
            REG_WIN1V => ((io.gpu.win1.top as u16) << 8 | (io.gpu.win1.bottom as u16)),
            REG_WININ => {
                ((io.gpu.win1.flags.bits() as u16) << 8) | (io.gpu.win0.flags.bits() as u16)
            }
            REG_WINOUT => {
                ((io.gpu.winobj_flags.bits() as u16) << 8) | (io.gpu.winout_flags.bits() as u16)
            }
            REG_BLDCNT => io.gpu.bldcnt.0,
            REG_BLDALPHA => io.gpu.bldalpha.0,

            REG_IME => io.intc.interrupt_master_enable as u16,
            REG_IE => io.intc.interrupt_enable.0 as u16,
            REG_IF => io.intc.interrupt_flags.0 as u16,

            REG_TM0CNT_L..=REG_TM3CNT_H => io.timers.handle_read(io_addr),

            SOUND_BASE..=SOUND_END => io.sound.handle_read(io_addr),
            REG_DMA0CNT_H => io.dmac.channels[0].ctrl.0,
            REG_DMA1CNT_H => io.dmac.channels[1].ctrl.0,
            REG_DMA2CNT_H => io.dmac.channels[2].ctrl.0,
            REG_DMA3CNT_H => io.dmac.channels[3].ctrl.0,

            REG_WAITCNT => io.waitcnt.0,

            REG_POSTFLG => io.post_boot_flag as u16,
            REG_HALTCNT => 0,
            REG_KEYINPUT => io.keyinput as u16,

            _ => {
                trace!(
                    "Unimplemented read from {:x} {}",
                    io_addr,
                    io_reg_string(io_addr)
                );
                0
            }
        }
    }

    fn read_8(&self, addr: Addr) -> u8 {
        let t = self.read_16(addr & !1);
        if addr & 1 != 0 {
            (t >> 8) as u8
        } else {
            t as u8
        }
    }

    fn write_16(&mut self, addr: Addr, value: u16) {
        let mut io = self;
        if addr > 0x0800 {
            return;
        }
        let io_addr = addr + IO_BASE;
        match io_addr {
            REG_DISPCNT => io.gpu.dispcnt.0 = value,
            REG_DISPSTAT => io.gpu.dispstat.0 = value | (io.gpu.dispstat.0 & 7),
            REG_BG0CNT => io.gpu.backgrounds[0].bgcnt.0 = value,
            REG_BG1CNT => io.gpu.backgrounds[1].bgcnt.0 = value,
            REG_BG2CNT => io.gpu.backgrounds[2].bgcnt.0 = value,
            REG_BG3CNT => io.gpu.backgrounds[3].bgcnt.0 = value,
            REG_BG0HOFS => io.gpu.backgrounds[0].bghofs = value & 0x1ff,
            REG_BG0VOFS => io.gpu.backgrounds[0].bgvofs = value & 0x1ff,
            REG_BG1HOFS => io.gpu.backgrounds[1].bghofs = value & 0x1ff,
            REG_BG1VOFS => io.gpu.backgrounds[1].bgvofs = value & 0x1ff,
            REG_BG2HOFS => io.gpu.backgrounds[2].bghofs = value & 0x1ff,
            REG_BG2VOFS => io.gpu.backgrounds[2].bgvofs = value & 0x1ff,
            REG_BG3HOFS => io.gpu.backgrounds[3].bghofs = value & 0x1ff,
            REG_BG3VOFS => io.gpu.backgrounds[3].bgvofs = value & 0x1ff,
            REG_BG2X_L | REG_BG3X_L => {
                let index = (io_addr - REG_BG2X_L) / 0x10;
                let t = io.gpu.bg_aff[index as usize].x as u32;
                io.gpu.bg_aff[index as usize].x = ((t & 0xffff0000) + (value as u32)) as i32;
                if !io.gpu.state.is_vblank() {
                    io.gpu.bg_aff[index as usize].internal_x = io.gpu.bg_aff[index as usize].x;
                }
            }
            REG_BG2Y_L | REG_BG3Y_L => {
                let index = (io_addr - REG_BG2X_L) / 0x10;
                let t = io.gpu.bg_aff[index as usize].y as u32;
                io.gpu.bg_aff[index as usize].y = ((t & 0xffff0000) + (value as u32)) as i32;
                if !io.gpu.state.is_vblank() {
                    io.gpu.bg_aff[index as usize].internal_y = io.gpu.bg_aff[index as usize].y;
                }
            }
            REG_BG2X_H | REG_BG3X_H => {
                let index = (io_addr - REG_BG2X_L) / 0x10;
                let t = io.gpu.bg_aff[index as usize].x;
                io.gpu.bg_aff[index as usize].x =
                    (t & 0xffff) | ((sign_extend_i32((value & 0xfff) as i32, 12)) << 16);
                if !io.gpu.state.is_vblank() {
                    io.gpu.bg_aff[index as usize].internal_x = io.gpu.bg_aff[index as usize].x;
                }
            }
            REG_BG2Y_H | REG_BG3Y_H => {
                let index = (io_addr - REG_BG2X_L) / 0x10;
                let t = io.gpu.bg_aff[index as usize].y;
                io.gpu.bg_aff[index as usize].y =
                    (t & 0xffff) | ((sign_extend_i32((value & 0xfff) as i32, 12)) << 16);
                if !io.gpu.state.is_vblank() {
                    io.gpu.bg_aff[index as usize].internal_y = io.gpu.bg_aff[index as usize].y;
                }
            }
            REG_BG2PA => io.gpu.bg_aff[0].pa = value as i16,
            REG_BG2PB => io.gpu.bg_aff[0].pb = value as i16,
            REG_BG2PC => io.gpu.bg_aff[0].pc = value as i16,
            REG_BG2PD => io.gpu.bg_aff[0].pd = value as i16,
            REG_BG3PA => io.gpu.bg_aff[1].pa = value as i16,
            REG_BG3PB => io.gpu.bg_aff[1].pb = value as i16,
            REG_BG3PC => io.gpu.bg_aff[1].pc = value as i16,
            REG_BG3PD => io.gpu.bg_aff[1].pd = value as i16,
            REG_WIN0H => {
                let right = value & 0xff;
                let left = value >> 8;
                io.gpu.win0.right = right as u8;
                io.gpu.win0.left = left as u8;
            }
            REG_WIN1H => {
                let right = value & 0xff;
                let left = value >> 8;
                io.gpu.win1.right = right as u8;
                io.gpu.win1.left = left as u8;
            }
            REG_WIN0V => {
                let bottom = value & 0xff;
                let top = value >> 8;
                io.gpu.win0.bottom = bottom as u8;
                io.gpu.win0.top = top as u8;
            }
            REG_WIN1V => {
                let bottom = value & 0xff;
                let top = value >> 8;
                io.gpu.win1.bottom = bottom as u8;
                io.gpu.win1.top = top as u8;
            }
            REG_WININ => {
                let value = value & !0xc0c0;
                io.gpu.win0.flags = WindowFlags::from(value & 0xff);
                io.gpu.win1.flags = WindowFlags::from(value >> 8);
            }
            REG_WINOUT => {
                let value = value & !0xc0c0;
                io.gpu.winout_flags = WindowFlags::from(value & 0xff);
                io.gpu.winobj_flags = WindowFlags::from(value >> 8);
            }
            REG_MOSAIC => io.gpu.mosaic.0 = value,
            REG_BLDCNT => io.gpu.bldcnt.0 = value,
            REG_BLDALPHA => io.gpu.bldalpha.0 = value,
            REG_BLDY => io.gpu.bldy = value & 0b11111,

            REG_IME => io.intc.interrupt_master_enable = value != 0,
            REG_IE => io.intc.interrupt_enable.0 = value,
            REG_IF => io.intc.interrupt_flags.0 &= !value,

            REG_TM0CNT_L..=REG_TM3CNT_H => io.timers.handle_write(io_addr, value),

            SOUND_BASE..=SOUND_END => {
                io.sound.handle_write(io_addr, value);
            }

            DMA_BASE..=REG_DMA3CNT_H => {
                let ofs = io_addr - DMA_BASE;
                let channel_id = (ofs / 12) as usize;
                io.dmac.write_16(channel_id, ofs % 12, value)
            }

            REG_WAITCNT => {
                io.waitcnt.0 = value;
                (*io.sysbus_ptr).on_waitcnt_written(io.waitcnt);
            }

            REG_POSTFLG => io.post_boot_flag = value != 0,
            REG_HALTCNT => {
                if value & 0x80 != 0 {
                    io.haltcnt = HaltState::Stop;
                    panic!("Can't handle HaltCtrl == Stop yet");
                } else {
                    io.haltcnt = HaltState::Halt;
                }
            }

            _ => {
                trace!(
                    "Unimplemented write to {:x} {}",
                    io_addr,
                    io_reg_string(io_addr)
                );
            }
        }
    }

    fn write_8(&mut self, addr: Addr, value: u8) {
        match addr + IO_BASE {
            /* FIFO_A */
            0x0400_00A0 | 0x0400_00A1 | 0x0400_00A2 | 0x0400_00A3 => {
                self.sound.write_fifo(0, value as i8)
            }
            /* FIFO_B */
            0x0400_00A4 | 0x0400_00A5 | 0x0400_00A6 | 0x0400_00A7 => {
                self.sound.write_fifo(1, value as i8)
            }
            _ => {
                let t = self.read_16(addr & !1);
                let t = if addr & 1 != 0 {
                    (t & 0xff) | (value as u16) << 8
                } else {
                    (t & 0xff00) | (value as u16)
                };
                self.write_16(addr & !1, t);
            }
        }
    }
}

bitfield! {
    #[derive(Serialize, Deserialize, Default, Copy, Clone, PartialEq)]
    pub struct WaitControl(u16);
    impl Debug;
    u16;
    pub sram_wait_control, _:      1, 0;
    pub ws0_first_access, _:       3, 2;
    pub ws0_second_access, _:      4, 4;
    pub ws1_first_access, _:       6, 5;
    pub ws1_second_access, _:      7, 7;
    pub ws2_first_access, _:       9, 8;
    pub ws2_second_access, _:      10, 10;
    #[allow(non_snake_case)]
    PHI_terminal_output, _:    12, 11;
    prefetch, _:               14;
}

#[rustfmt::skip]
pub mod consts {
    use super::*;

    pub const IO_BASE: Addr = 0x0400_0000;

    pub const REG_DISPCNT: Addr = 0x0400_0000;      //  2    R/W    LCD Control
    pub const REG_GREENSWAP: Addr = 0x0400_0002;    //  2    R/W    Undocumented - Green Swap
    pub const REG_DISPSTAT: Addr = 0x0400_0004;     //  2    R/W    General LCD Status (STAT,LYC)
    pub const REG_VCOUNT: Addr = 0x0400_0006;       //  2    R      Vertical Counter (LY)
    pub const REG_BG0CNT: Addr = 0x0400_0008;       //  2    R/W    BG0 Control
    pub const REG_BG1CNT: Addr = 0x0400_000A;       //  2    R/W    BG1 Control
    pub const REG_BG2CNT: Addr = 0x0400_000C;       //  2    R/W    BG2 Control
    pub const REG_BG3CNT: Addr = 0x0400_000E;       //  2    R/W    BG3 Control
    pub const REG_BG0HOFS: Addr = 0x0400_0010;      //  2    W      BG0 X-Offset
    pub const REG_BG0VOFS: Addr = 0x0400_0012;      //  2    W      BG0 Y-Offset
    pub const REG_BG1HOFS: Addr = 0x0400_0014;      //  2    W      BG1 X-Offset
    pub const REG_BG1VOFS: Addr = 0x0400_0016;      //  2    W      BG1 Y-Offset
    pub const REG_BG2HOFS: Addr = 0x0400_0018;      //  2    W      BG2 X-Offset
    pub const REG_BG2VOFS: Addr = 0x0400_001A;      //  2    W      BG2 Y-Offset
    pub const REG_BG3HOFS: Addr = 0x0400_001C;      //  2    W      BG3 X-Offset
    pub const REG_BG3VOFS: Addr = 0x0400_001E;      //  2    W      BG3 Y-Offset
    pub const REG_BG2PA: Addr = 0x0400_0020;        //  2    W      BG2 Rotation/Scaling Parameter A (dx)
    pub const REG_BG2PB: Addr = 0x0400_0022;        //  2    W      BG2 Rotation/Scaling Parameter B (dmx)
    pub const REG_BG2PC: Addr = 0x0400_0024;        //  2    W      BG2 Rotation/Scaling Parameter C (dy)
    pub const REG_BG2PD: Addr = 0x0400_0026;        //  2    W      BG2 Rotation/Scaling Parameter D (dmy)
    pub const REG_BG2X_L: Addr = 0x0400_0028;       //  4    W      BG2 Reference Point X-Coordinate, lower 16 bit
    pub const REG_BG2X_H: Addr = 0x0400_002A;       //  4    W      BG2 Reference Point X-Coordinate, upper 16 bit
    pub const REG_BG2Y_L: Addr = 0x0400_002C;       //  4    W      BG2 Reference Point Y-Coordinate, lower 16 bit
    pub const REG_BG2Y_H: Addr = 0x0400_002E;       //  4    W      BG2 Reference Point Y-Coordinate, upper 16 bit
    pub const REG_BG3PA: Addr = 0x0400_0030;        //  2    W      BG3 Rotation/Scaling Parameter A (dx)
    pub const REG_BG3PB: Addr = 0x0400_0032;        //  2    W      BG3 Rotation/Scaling Parameter B (dmx)
    pub const REG_BG3PC: Addr = 0x0400_0034;        //  2    W      BG3 Rotation/Scaling Parameter C (dy)
    pub const REG_BG3PD: Addr = 0x0400_0036;        //  2    W      BG3 Rotation/Scaling Parameter D (dmy)
    pub const REG_BG3X_L: Addr = 0x0400_0038;       //  4    W      BG3 Reference Point X-Coordinate, lower 16 bit
    pub const REG_BG3X_H: Addr = 0x0400_003A;       //  4    W      BG3 Reference Point X-Coordinate, upper 16 bit
    pub const REG_BG3Y_L: Addr = 0x0400_003C;       //  4    W      BG3 Reference Point Y-Coordinate, lower 16 bit
    pub const REG_BG3Y_H: Addr = 0x0400_003E;       //  4    W      BG3 Reference Point Y-Coordinate, upper 16 bit
    pub const REG_WIN0H: Addr = 0x0400_0040;        //  2    W      Window 0 Horizontal Dimensions
    pub const REG_WIN1H: Addr = 0x0400_0042;        //  2    W      Window 1 Horizontal Dimensions
    pub const REG_WIN0V: Addr = 0x0400_0044;        //  2    W      Window 0 Vertical Dimensions
    pub const REG_WIN1V: Addr = 0x0400_0046;        //  2    W      Window 1 Vertical Dimensions
    pub const REG_WININ: Addr = 0x0400_0048;        //  2    R/W    Inside of Window 0 and 1
    pub const REG_WINOUT: Addr = 0x0400_004A;       //  2    R/W    Inside of OBJ Window & Outside of Windows
    pub const REG_MOSAIC: Addr = 0x0400_004C;       //  2    W      Mosaic Size
    pub const REG_BLDCNT: Addr = 0x0400_0050;       //  2    R/W    Color Special Effects Selection
    pub const REG_BLDALPHA: Addr = 0x0400_0052;     //  2    R/W    Alpha Blending Coefficients
    pub const REG_BLDY: Addr = 0x0400_0054;         //  2    W      Brightness (Fade-In/Out) Coefficient
    pub const SOUND_BASE: Addr = REG_SOUND1CNT_L;
    pub const REG_SOUND1CNT_L: Addr = 0x0400_0060;  //  2  R/W      Channel 1 Sweep register       (NR10)
    pub const REG_SOUND1CNT_H: Addr = 0x0400_0062;  //  2  R/W      Channel 1 Duty/Length/Envelope (NR11, NR12)
    pub const REG_SOUND1CNT_X: Addr = 0x0400_0064;  //  2  R/W      Channel 1 Frequency/Control    (NR13, NR14)
    pub const REG_SOUND2CNT_L: Addr = 0x0400_0068;  //  2  R/W      Channel 2 Duty/Length/Envelope (NR21, NR22)
    pub const REG_SOUND2CNT_H: Addr = 0x0400_006C;  //  2  R/W      Channel 2 Frequency/Control    (NR23, NR24)
    pub const REG_SOUND3CNT_L: Addr = 0x0400_0070;  //  2  R/W      Channel 3 Stop/Wave RAM select (NR30)
    pub const REG_SOUND3CNT_H: Addr = 0x0400_0072;  //  2  R/W      Channel 3 Length/Volume        (NR31, NR32)
    pub const REG_SOUND3CNT_X: Addr = 0x0400_0074;  //  2  R/W      Channel 3 Frequency/Control    (NR33, NR34)
    pub const REG_SOUND4CNT_L: Addr = 0x0400_0078;  //  2  R/W      Channel 4 Length/Envelope      (NR41, NR42)
    pub const REG_SOUND4CNT_H: Addr = 0x0400_007C;  //  2  R/W      Channel 4 Frequency/Control    (NR43, NR44)
    pub const REG_SOUNDCNT_L: Addr = 0x0400_0080;   //  2  R/W      Control Stereo/Volume/Enable   (NR50, NR51)
    pub const REG_SOUNDCNT_H: Addr = 0x0400_0082;   //  2  R/W      Control Mixing/DMA Control
    pub const REG_SOUNDCNT_X: Addr = 0x0400_0084;   //  2  R/W      Control Sound on/off           (NR52)
    pub const REG_SOUNDBIAS: Addr = 0x0400_0088;    //  2  BIOS     Sound PWM Control
    pub const REG_WAVE_RAM: Addr = 0x0400_0090;     //              Channel 3 Wave Pattern RAM (2 banks!!)
    pub const REG_FIFO_A: Addr = 0x0400_00A0;       //  4    W      Channel A FIFO, Data 0-3
    pub const REG_FIFO_B: Addr = 0x0400_00A4;       //  4    W      Channel B FIFO, Data 0-3
    pub const SOUND_END: Addr = REG_FIFO_B+4;
    pub const DMA_BASE: Addr = REG_DMA0SAD;
    pub const REG_DMA0SAD: Addr = 0x0400_00B0;      //  4    W      DMA 0 Source Address
    pub const REG_DMA0DAD: Addr = 0x0400_00B4;      //  4    W      DMA 0 Destination Address
    pub const REG_DMA0CNT_L: Addr = 0x0400_00B8;    //  2    W      DMA 0 Word Count
    pub const REG_DMA0CNT_H: Addr = 0x0400_00BA;    //  2    R/W    DMA 0 Control
    pub const REG_DMA1SAD: Addr = 0x0400_00BC;      //  4    W      DMA 1 Source Address
    pub const REG_DMA1DAD: Addr = 0x0400_00C0;      //  4    W      DMA 1 Destination Address
    pub const REG_DMA1CNT_L: Addr = 0x0400_00C4;    //  2    W      DMA 1 Word Count
    pub const REG_DMA1CNT_H: Addr = 0x0400_00C6;    //  2    R/W    DMA 1 Control
    pub const REG_DMA2SAD: Addr = 0x0400_00C8;      //  4    W      DMA 2 Source Address
    pub const REG_DMA2DAD: Addr = 0x0400_00CC;      //  4    W      DMA 2 Destination Address
    pub const REG_DMA2CNT_L: Addr = 0x0400_00D0;    //  2    W      DMA 2 Word Count
    pub const REG_DMA2CNT_H: Addr = 0x0400_00D2;    //  2    R/W    DMA 2 Control
    pub const REG_DMA3SAD: Addr = 0x0400_00D4;      //  4    W      DMA 3 Source Address
    pub const REG_DMA3DAD: Addr = 0x0400_00D8;      //  4    W      DMA 3 Destination Address
    pub const REG_DMA3CNT_L: Addr = 0x0400_00DC;    //  2    W      DMA 3 Word Count
    pub const REG_DMA3CNT_H: Addr = 0x0400_00DE;    //  2    R/W    DMA 3 Control
    pub const REG_TM0CNT_L: Addr = 0x0400_0100;     //  2    R/W    Timer 0 Counter/Reload
    pub const REG_TM0CNT_H: Addr = 0x0400_0102;     //  2    R/W    Timer 0 Control
    pub const REG_TM1CNT_L: Addr = 0x0400_0104;     //  2    R/W    Timer 1 Counter/Reload
    pub const REG_TM1CNT_H: Addr = 0x0400_0106;     //  2    R/W    Timer 1 Control
    pub const REG_TM2CNT_L: Addr = 0x0400_0108;     //  2    R/W    Timer 2 Counter/Reload
    pub const REG_TM2CNT_H: Addr = 0x0400_010A;     //  2    R/W    Timer 2 Control
    pub const REG_TM3CNT_L: Addr = 0x0400_010C;     //  2    R/W    Timer 3 Counter/Reload
    pub const REG_TM3CNT_H: Addr = 0x0400_010E;     //  2    R/W    Timer 3 Control
    // pub const REG_SIODATA32: Addr = 0x0400_0120;    //  4    R/W    SIO Data (Normal-32bit Mode; shared with below)
    // pub const REG_SIOMULTI0: Addr = 0x0400_0120;    //  2    R/W    SIO Data 0 (Parent)    (Multi-Player Mode)
    // pub const REG_SIOMULTI1: Addr = 0x0400_0122;    //  2    R/W    SIO Data 1 (1st Child) (Multi-Player Mode)
    // pub const REG_SIOMULTI2: Addr = 0x0400_0124;    //  2    R/W    SIO Data 2 (2nd Child) (Multi-Player Mode)
    // pub const REG_SIOMULTI3: Addr = 0x0400_0126;    //  2    R/W    SIO Data 3 (3rd Child) (Multi-Player Mode)
    // pub const REG_SIOCNT: Addr = 0x0400_0128;       //  2    R/W    SIO Control Register
    // pub const REG_SIOMLT_SEND: Addr = 0x0400_012A;  //  2    R/W    SIO Data (Local of MultiPlayer; shared below)
    // pub const REG_SIODATA8: Addr = 0x0400_012A;     //  2    R/W    SIO Data (Normal-8bit and UART Mode)
    pub const REG_KEYINPUT: Addr = 0x0400_0130;     //  2    R      Key Status
    pub const REG_KEYCNT: Addr = 0x0400_0132;       //  2    R/W    Key Interrupt Control
    pub const REG_RCNT: Addr = 0x0400_0134;         //  2    R/W    SIO Mode Select/General Purpose Data
    pub const REG_IR: Addr = 0x0400_0136;           //  -    -      Ancient - Infrared Register (Prototypes only)
    pub const REG_JOYCNT: Addr = 0x0400_0140;       //  2    R/W    SIO JOY Bus Control
    pub const REG_JOY_RECV: Addr = 0x0400_0150;     //  4    R/W    SIO JOY Bus Receive Data
    pub const REG_JOY_TRANS: Addr = 0x0400_0154;    //  4    R/W    SIO JOY Bus Transmit Data
    pub const REG_JOYSTAT: Addr = 0x0400_0158;      //  2    R/?    SIO JOY Bus Receive Status
    pub const REG_IE: Addr = 0x0400_0200;           //  2    R/W    Interrupt Enable Register
    pub const REG_IF: Addr = 0x0400_0202;           //  2    R/W    Interrupt Request Flags / IRQ Acknowledge
    pub const REG_WAITCNT: Addr = 0x0400_0204;      //  2    R/W    Game Pak Waitstate Control
    pub const REG_IME: Addr = 0x0400_0208;          //  2    R/W    Interrupt Master Enable Register
    pub const REG_POSTFLG: Addr = 0x0400_0300;      //  1    R/W    Undocumented - Post Boot Flag
    pub const REG_HALTCNT: Addr = 0x0400_0301;      //  1    W      Undocumented - Power Down Control
}

pub fn io_reg_string(addr: u32) -> &'static str {
    match addr {
        REG_DISPCNT => "REG_DISPCNT",
        REG_DISPSTAT => "REG_DISPSTAT",
        REG_VCOUNT => "REG_VCOUNT",
        REG_BG0CNT => "REG_BG0CNT",
        REG_BG1CNT => "REG_BG1CNT",
        REG_BG2CNT => "REG_BG2CNT",
        REG_BG3CNT => "REG_BG3CNT",
        REG_BG0HOFS => "REG_BG0HOFS",
        REG_BG0VOFS => "REG_BG0VOFS",
        REG_BG1HOFS => "REG_BG1HOFS",
        REG_BG1VOFS => "REG_BG1VOFS",
        REG_BG2HOFS => "REG_BG2HOFS",
        REG_BG2VOFS => "REG_BG2VOFS",
        REG_BG3HOFS => "REG_BG3HOFS",
        REG_BG3VOFS => "REG_BG3VOFS",
        REG_BG2PA => "REG_BG2PA",
        REG_BG2PB => "REG_BG2PB",
        REG_BG2PC => "REG_BG2PC",
        REG_BG2PD => "REG_BG2PD",
        REG_BG2X_L => "REG_BG2X_L",
        REG_BG2X_H => "REG_BG2X_H",
        REG_BG2Y_L => "REG_BG2Y_L",
        REG_BG2Y_H => "REG_BG2Y_H",
        REG_BG3PA => "REG_BG3PA",
        REG_BG3PB => "REG_BG3PB",
        REG_BG3PC => "REG_BG3PC",
        REG_BG3PD => "REG_BG3PD",
        REG_BG3X_L => "REG_BG3X_L",
        REG_BG3X_H => "REG_BG3X_H",
        REG_BG3Y_L => "REG_BG3Y_L",
        REG_BG3Y_H => "REG_BG3Y_H",
        REG_WIN0H => "REG_WIN0H",
        REG_WIN1H => "REG_WIN1H",
        REG_WIN0V => "REG_WIN0V",
        REG_WIN1V => "REG_WIN1V",
        REG_WININ => "REG_WININ",
        REG_WINOUT => "REG_WINOUT",
        REG_MOSAIC => "REG_MOSAIC",
        REG_BLDCNT => "REG_BLDCNT",
        REG_BLDALPHA => "REG_BLDALPHA",
        REG_BLDY => "REG_BLDY",
        REG_SOUND1CNT_L => "REG_SOUND1CNT_L",
        REG_SOUND1CNT_H => "REG_SOUND1CNT_H",
        REG_SOUND1CNT_X => "REG_SOUND1CNT_X",
        REG_SOUND2CNT_L => "REG_SOUND2CNT_L",
        REG_SOUND2CNT_H => "REG_SOUND2CNT_H",
        REG_SOUND3CNT_L => "REG_SOUND3CNT_L",
        REG_SOUND3CNT_H => "REG_SOUND3CNT_H",
        REG_SOUND3CNT_X => "REG_SOUND3CNT_X",
        REG_SOUND4CNT_L => "REG_SOUND4CNT_L",
        REG_SOUND4CNT_H => "REG_SOUND4CNT_H",
        REG_SOUNDCNT_L => "REG_SOUNDCNT_L",
        REG_SOUNDCNT_H => "REG_SOUNDCNT_H",
        REG_SOUNDCNT_X => "REG_SOUNDCNT_X",
        REG_SOUNDBIAS => "REG_SOUNDBIAS",
        REG_WAVE_RAM => "REG_WAVE_RAM",
        REG_FIFO_A => "REG_FIFO_A",
        REG_FIFO_B => "REG_FIFO_B",
        REG_DMA0SAD => "REG_DMA0SAD",
        REG_DMA0DAD => "REG_DMA0DAD",
        REG_DMA0CNT_L => "REG_DMA0CNT_L",
        REG_DMA0CNT_H => "REG_DMA0CNT_H",
        REG_DMA1SAD => "REG_DMA1SAD",
        REG_DMA1DAD => "REG_DMA1DAD",
        REG_DMA1CNT_L => "REG_DMA1CNT_L",
        REG_DMA1CNT_H => "REG_DMA1CNT_H",
        REG_DMA2SAD => "REG_DMA2SAD",
        REG_DMA2DAD => "REG_DMA2DAD",
        REG_DMA2CNT_L => "REG_DMA2CNT_L",
        REG_DMA2CNT_H => "REG_DMA2CNT_H",
        REG_DMA3SAD => "REG_DMA3SAD",
        REG_DMA3DAD => "REG_DMA3DAD",
        REG_DMA3CNT_L => "REG_DMA3CNT_L",
        REG_DMA3CNT_H => "REG_DMA3CNT_H",
        REG_TM0CNT_L => "REG_TM0CNT_L",
        REG_TM0CNT_H => "REG_TM0CNT_H",
        REG_TM1CNT_L => "REG_TM1CNT_L",
        REG_TM1CNT_H => "REG_TM1CNT_H",
        REG_TM2CNT_L => "REG_TM2CNT_L",
        REG_TM2CNT_H => "REG_TM2CNT_H",
        REG_TM3CNT_L => "REG_TM3CNT_L",
        REG_TM3CNT_H => "REG_TM3CNT_H",
        // REG_SIODATA32 => "REG_SIODATA32",
        // REG_SIOMULTI0 => "REG_SIOMULTI0",
        // REG_SIOMULTI1 => "REG_SIOMULTI1",
        // REG_SIOMULTI2 => "REG_SIOMULTI2",
        // REG_SIOMULTI3 => "REG_SIOMULTI3",
        // REG_SIOCNT => "REG_SIOCNT",
        // REG_SIOMLT_SEND => "REG_SIOMLT_SEND",
        // REG_SIODATA8 => "REG_SIODATA8",
        REG_KEYINPUT => "REG_KEYINPUT",
        REG_KEYCNT => "REG_KEYCNT",
        REG_RCNT => "REG_RCNT",
        REG_IR => "REG_IR",
        REG_JOYCNT => "REG_JOYCNT",
        REG_JOY_RECV => "REG_JOY_RECV",
        REG_JOY_TRANS => "REG_JOY_TRANS",
        REG_JOYSTAT => "REG_JOYSTAT",
        REG_IE => "REG_IE",
        REG_IF => "REG_IF",
        REG_WAITCNT => "REG_WAITCNT",
        REG_IME => "REG_IME",
        REG_POSTFLG => "REG_POSTFLG",
        REG_HALTCNT => "REG_HALTCNT",
        _ => "UNKNOWN",
    }
}

fn sign_extend_i32(value: i32, size: u32) -> i32 {
    let shift = 32 - size;
    ((value << shift) as i32) >> shift
}
