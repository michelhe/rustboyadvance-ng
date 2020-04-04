use super::super::super::SysBus;
use super::super::Core;
use super::super::CpuAction;
use super::{ArmFormat, ArmInstruction};

use bit::BitIndex;

pub type ArmInstructionHandler = fn(&mut Core, &mut SysBus, &ArmInstruction) -> CpuAction;

impl From<ArmFormat> for ArmInstructionHandler {
    fn from(arm_fmt: ArmFormat) -> ArmInstructionHandler {
        match arm_fmt {
            ArmFormat::BranchExchange => Core::exec_arm_bx,
            ArmFormat::BranchLink => Core::exec_arm_b_bl,
            ArmFormat::DataProcessing => Core::exec_arm_data_processing,
            ArmFormat::SoftwareInterrupt => Core::exec_arm_swi,
            ArmFormat::SingleDataTransfer => Core::exec_arm_ldr_str,
            ArmFormat::HalfwordDataTransferImmediateOffset => Core::exec_arm_ldr_str_hs,
            ArmFormat::HalfwordDataTransferRegOffset => Core::exec_arm_ldr_str_hs,
            ArmFormat::BlockDataTransfer => Core::exec_arm_ldm_stm,
            ArmFormat::Multiply => Core::exec_arm_mul_mla,
            ArmFormat::MultiplyLong => Core::exec_arm_mull_mlal,
            ArmFormat::SingleDataSwap => Core::exec_arm_swp,
            _ => Core::arm_undefined,
        }
    }
}

pub struct ArmInstructionInfo {
    pub fmt: ArmFormat,
    pub handler_fn: ArmInstructionHandler,
}

impl ArmInstructionInfo {
    fn new(fmt: ArmFormat, handler_fn: ArmInstructionHandler) -> ArmInstructionInfo {
        ArmInstructionInfo { fmt, handler_fn }
    }
}

#[inline(always)]
pub fn arm_insn_hash(insn: u32) -> usize {
    (((insn >> 16) & 0xff0) | ((insn >> 4) & 0x00f)) as usize
}

impl From<u32> for ArmFormat {
    fn from(i: u32) -> ArmFormat {
        use ArmFormat::*;

        // match i.bit_range(26..28) {
        //     0b00 => {
        //         match (i.bit_range(23..26), i.bit(22) as u32, i.bit_range(20..22), i.bit_range(4..8)) {
        //             (0b000, 0b0, , _, 0b1001) => Multiply,
        //             (0b001, _,  _, 0b1001) => MultiplyLong,
        //             (0b010, _,  0b00, 0b1001) => SingleDataSwap,
        //             (0b010, 0b0, 0b10, 0b0001) => BranchExchange,

        //             _ => DataProcessing
        //         }
        //     }
        //     0b01 => {
        //         if i.bit(4) {
        //             Undefined
        //         } else {
        //             SingleDataTransfer
        //         }
        //     }
        //     0b10 => {

        //     }
        //     0b11 {

        //     }
        // }

        if (0x0ff0_00f0 & i) == 0x0120_0010 {
            BranchExchange
        } else if (0x0e00_0000 & i) == 0x0a00_0000 {
            BranchLink
        } else if (0xe000_0010 & i) == 0x0600_0000 {
            Undefined
        } else if (0x0fb0_0ff0 & i) == 0x0100_0090 {
            SingleDataSwap
        } else if (0x0fc0_00f0 & i) == 0x0000_0090 {
            Multiply
        } else if (0x0f80_00f0 & i) == 0x0080_0090 {
            MultiplyLong
        } else if (0x0c00_0000 & i) == 0x0400_0000 {
            SingleDataTransfer
        } else if (0x0e40_0F90 & i) == 0x0000_0090 {
            HalfwordDataTransferRegOffset
        } else if (0x0e40_0090 & i) == 0x0040_0090 {
            HalfwordDataTransferImmediateOffset
        } else if (0x0e00_0000 & i) == 0x0800_0000 {
            BlockDataTransfer
        } else if (0x0f00_0000 & i) == 0x0f00_0000 {
            SoftwareInterrupt
        } else if (0x0c00_0000 & i) == 0x0000_0000 {
            DataProcessing
        } else {
            Undefined
        }
    }
}

lazy_static! {

    pub static ref ARM_FN_LUT: [ArmInstructionHandler; 256] = {

        use std::mem::{self, MaybeUninit};

        let mut lut: [MaybeUninit<ArmInstructionHandler>; 256] = unsafe {
            MaybeUninit::uninit().assume_init()
        };

        for i in 0..256 {
            lut[i] = MaybeUninit::new(Core::arm_undefined);
        }

        lut[ArmFormat::BranchExchange as usize] = MaybeUninit::new(Core::exec_arm_bx);
        lut[ArmFormat::BranchLink as usize] = MaybeUninit::new(Core::exec_arm_b_bl);
        lut[ArmFormat::DataProcessing as usize] = MaybeUninit::new(Core::exec_arm_data_processing);
        lut[ArmFormat::SoftwareInterrupt as usize] = MaybeUninit::new(Core::exec_arm_swi);
        lut[ArmFormat::SingleDataTransfer as usize] = MaybeUninit::new(Core::exec_arm_ldr_str);
        lut[ArmFormat::HalfwordDataTransferImmediateOffset as usize] = MaybeUninit::new(Core::exec_arm_ldr_str_hs);
        lut[ArmFormat::HalfwordDataTransferRegOffset as usize] = MaybeUninit::new(Core::exec_arm_ldr_str_hs);
        lut[ArmFormat::BlockDataTransfer as usize] = MaybeUninit::new(Core::exec_arm_ldm_stm);
        lut[ArmFormat::MoveFromStatus as usize] = MaybeUninit::new(Core::exec_arm_mrs);
        lut[ArmFormat::MoveToStatus as usize] = MaybeUninit::new(Core::exec_arm_msr_reg);
        lut[ArmFormat::MoveToFlags as usize] = MaybeUninit::new(Core::exec_arm_msr_flags);
        lut[ArmFormat::Multiply as usize] = MaybeUninit::new(Core::exec_arm_mul_mla);
        lut[ArmFormat::MultiplyLong as usize] = MaybeUninit::new(Core::exec_arm_mull_mlal);
        lut[ArmFormat::SingleDataSwap as usize] = MaybeUninit::new(Core::exec_arm_swp);
        lut[ArmFormat::Undefined as usize] = MaybeUninit::new(Core::arm_undefined);

        // Everything is initialized. Transmute the array to the
        // initialized type.
        unsafe { mem::transmute::<_, [ArmInstructionHandler; 256]>(lut) }
    };

    // there are 0xfff different hashes
    pub static ref ARM_LUT: [u8; 4096] = {

        debug!("generating ARM lookup table");

        use std::mem::{self, MaybeUninit};

        let mut lut: [MaybeUninit<u8>; 4096] = unsafe {
            MaybeUninit::uninit().assume_init()
        };

        for i in 0..4096 {
            let x = ((i & 0xff0) << 16) | ((i & 0x00f) << 4);
            let fmt = ArmFormat::from(x);
            lut[i as usize] = MaybeUninit::new(fmt as u8);
        }

        // Everything is initialized. Transmute the array to the
        // initialized type.
        unsafe { mem::transmute::<_, [u8; 4096]>(lut) }
    };
}
