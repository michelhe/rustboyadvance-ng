use super::super::super::SysBus;
use super::super::Core;
use super::super::CpuAction;
use super::super::InstructionDecoder;
use super::{ThumbFormat, ThumbInstruction};

pub type ThumbInstructionHandler = fn(&mut Core, &mut SysBus, &ThumbInstruction) -> CpuAction;

impl From<ThumbFormat> for ThumbInstructionHandler {
    fn from(thumb_fmt: ThumbFormat) -> ThumbInstructionHandler {
        match thumb_fmt {
            ThumbFormat::MoveShiftedReg => Core::exec_thumb_move_shifted_reg,
            ThumbFormat::AddSub => Core::exec_thumb_add_sub,
            ThumbFormat::DataProcessImm => Core::exec_thumb_data_process_imm,
            ThumbFormat::AluOps => Core::exec_thumb_alu_ops,
            ThumbFormat::HiRegOpOrBranchExchange => Core::exec_thumb_hi_reg_op_or_bx,
            ThumbFormat::LdrPc => Core::exec_thumb_ldr_pc,
            ThumbFormat::LdrStrRegOffset => Core::exec_thumb_ldr_str_reg_offset,
            ThumbFormat::LdrStrSHB => Core::exec_thumb_ldr_str_shb,
            ThumbFormat::LdrStrImmOffset => Core::exec_thumb_ldr_str_imm_offset,
            ThumbFormat::LdrStrHalfWord => Core::exec_thumb_ldr_str_halfword,
            ThumbFormat::LdrStrSp => Core::exec_thumb_ldr_str_sp,
            ThumbFormat::LoadAddress => Core::exec_thumb_load_address,
            ThumbFormat::AddSp => Core::exec_thumb_add_sp,
            ThumbFormat::PushPop => Core::exec_thumb_push_pop,
            ThumbFormat::LdmStm => Core::exec_thumb_ldm_stm,
            ThumbFormat::BranchConditional => Core::exec_thumb_branch_with_cond,
            ThumbFormat::Swi => Core::exec_thumb_swi,
            ThumbFormat::Branch => Core::exec_thumb_branch,
            ThumbFormat::BranchLongWithLink => Core::exec_thumb_branch_long_with_link,
            ThumbFormat::Undefined => Core::thumb_undefined,
        }
    }
}

pub struct ThumbInstructionInfo {
    pub fmt: ThumbFormat,
    pub handler_fn: ThumbInstructionHandler,
}

lazy_static! {
    pub static ref THUMB_LUT: [ThumbInstructionInfo; 1024] = {

        debug!("generating THUMB lookup table");

        use std::mem::{self, MaybeUninit};

        let mut lut: [MaybeUninit<ThumbInstructionInfo>; 1024] = unsafe {
            MaybeUninit::uninit().assume_init()
        };

        for i in 0..1024 {
            let insn = ThumbInstruction::decode(i << 6, 0);
            let info = ThumbInstructionInfo {
                fmt: insn.fmt,
                handler_fn: insn.fmt.into()
            };
            lut[i as usize] = MaybeUninit::new(info);
        }

        // Everything is initialized. Transmute the array to the
        // initialized type.
        unsafe { mem::transmute::<_, [ThumbInstructionInfo; 1024]>(lut) }
    };
}
