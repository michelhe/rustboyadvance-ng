use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

extern crate bit;
use bit::BitIndex;

// copied and slightly adjusted from src/core/arm7tdmi/thumb/mod.rs
fn thumb_decode(i: u16) -> &'static str {
    if i & 0xf800 == 0x1800 {
        "AddSub"
    } else if i & 0xe000 == 0x0000 {
        "MoveShiftedReg"
    } else if i & 0xe000 == 0x2000 {
        "DataProcessImm"
    } else if i & 0xfc00 == 0x4000 {
        "AluOps"
    } else if i & 0xfc00 == 0x4400 {
        "HiRegOpOrBranchExchange"
    } else if i & 0xf800 == 0x4800 {
        "LdrPc"
    } else if i & 0xf200 == 0x5000 {
        "LdrStrRegOffset"
    } else if i & 0xf200 == 0x5200 {
        "LdrStrSHB"
    } else if i & 0xe000 == 0x6000 {
        "LdrStrImmOffset"
    } else if i & 0xf000 == 0x8000 {
        "LdrStrHalfWord"
    } else if i & 0xf000 == 0x9000 {
        "LdrStrSp"
    } else if i & 0xf000 == 0xa000 {
        "LoadAddress"
    } else if i & 0xff00 == 0xb000 {
        "AddSp"
    } else if i & 0xf600 == 0xb400 {
        "PushPop"
    } else if i & 0xf000 == 0xc000 {
        "LdmStm"
    } else if i & 0xff00 == 0xdf00 {
        "Swi"
    } else if i & 0xf000 == 0xd000 {
        "BranchConditional"
    } else if i & 0xf800 == 0xe000 {
        "Branch"
    } else if i & 0xf000 == 0xf000 {
        "BranchLongWithLink"
    } else {
        "Undefined"
    }
}

trait BitAsInt<T: From<bool>>: BitIndex {
    fn ibit(&self, i: usize) -> T {
        self.bit(i).into()
    }
}

impl BitAsInt<u32> for u32 {}

/// Returns a string representation of rustboyadvance_ng::core::arm7tdmi::arm::ArmFormat enum member
/// # Arguments
/// * `i` - A 32bit ARM instruction
///
/// Decoding is according to this table from http://problemkaputt.de/gbatek.htm#ARMBinaryOpcodeFormat
/// ```
/// |..3 ..................2 ..................1 ..................0|
/// |1_0_9_8_7_6_5_4_3_2_1_0_9_8_7_6_5_4_3_2_1_0_9_8_7_6_5_4_3_2_1_0|
/// |_Cond__|0_0_0|___Op__|S|__Rn___|__Rd___|__Shift__|Typ|0|__Rm___| DataProc
/// |_Cond__|0_0_0|___Op__|S|__Rn___|__Rd___|__Rs___|0|Typ|1|__Rm___| DataProc
/// |_Cond__|0_0_1|___Op__|S|__Rn___|__Rd___|_Shift_|___Immediate___| DataProc
/// |_Cond__|0_0_1_1_0_0_1_0_0_0_0_0_1_1_1_1_0_0_0_0|_____Hint______| ARM11:Hint
/// |_Cond__|0_0_1_1_0|P|1|0|_Field_|__Rd___|_Shift_|___Immediate___| PSR Imm
/// |_Cond__|0_0_0_1_0|P|L|0|_Field_|__Rd___|0_0_0_0|0_0_0_0|__Rm___| PSR Reg
/// |_Cond__|0_0_0_1_0_0_1_0_1_1_1_1_1_1_1_1_1_1_1_1|0_0|L|1|__Rn___| BX,BLX
/// |1_1_1_0|0_0_0_1_0_0_1_0|_____immediate_________|0_1_1_1|_immed_| ARM9:BKPT
/// |_Cond__|0_0_0_1_0_1_1_0_1_1_1_1|__Rd___|1_1_1_1|0_0_0_1|__Rm___| ARM9:CLZ
/// |_Cond__|0_0_0_1_0|Op_|0|__Rn___|__Rd___|0_0_0_0|0_1_0_1|__Rm___| ARM9:QALU
/// |_Cond__|0_0_0_0_0_0|A|S|__Rd___|__Rn___|__Rs___|1_0_0_1|__Rm___| Multiply
/// |_Cond__|0_0_0_0_0_1_0_0|_RdHi__|_RdLo__|__Rs___|1_0_0_1|__Rm___| ARM11:UMAAL
/// |_Cond__|0_0_0_0_1|U|A|S|_RdHi__|_RdLo__|__Rs___|1_0_0_1|__Rm___| MulLong
/// |_Cond__|0_0_0_1_0|Op_|0|Rd/RdHi|Rn/RdLo|__Rs___|1|y|x|0|__Rm___| MulHalfARM9
/// |_Cond__|0_0_0_1_0|B|0_0|__Rn___|__Rd___|0_0_0_0|1_0_0_1|__Rm___| TransSwp12
/// |_Cond__|0_0_0_1_1|_Op__|__Rn___|__Rd___|1_1_1_1|1_0_0_1|__Rm___| ARM11:LDREX
/// |_Cond__|0_0_0|P|U|0|W|L|__Rn___|__Rd___|0_0_0_0|1|S|H|1|__Rm___| TransReg10
/// |_Cond__|0_0_0|P|U|1|W|L|__Rn___|__Rd___|OffsetH|1|S|H|1|OffsetL| TransImm10
/// |_Cond__|0_1_0|P|U|B|W|L|__Rn___|__Rd___|_________Offset________| TransImm9
/// |_Cond__|0_1_1|P|U|B|W|L|__Rn___|__Rd___|__Shift__|Typ|0|__Rm___| TransReg9
/// |_Cond__|0_1_1|________________xxx____________________|1|__xxx__| Undefined
/// |_Cond__|0_1_1|Op_|x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x|1|x_x_x_x| ARM11:Media
/// |1_1_1_1_0_1_0_1_0_1_1_1_1_1_1_1_1_1_1_1_0_0_0_0_0_0_0_1_1_1_1_1| ARM11:CLREX
/// |_Cond__|1_0_0|P|U|S|W|L|__Rn___|__________Register_List________| BlockTrans
/// |_Cond__|1_0_1|L|___________________Offset______________________| B,BL,BLX
/// |_Cond__|1_1_0|P|U|N|W|L|__Rn___|__CRd__|__CP#__|____Offset_____| CoDataTrans
/// |_Cond__|1_1_0_0_0_1_0|L|__Rn___|__Rd___|__CP#__|_CPopc_|__CRm__| CoRR ARM9
/// |_Cond__|1_1_1_0|_CPopc_|__CRn__|__CRd__|__CP#__|_CP__|0|__CRm__| CoDataOp
/// |_Cond__|1_1_1_0|CPopc|L|__CRn__|__Rd___|__CP#__|_CP__|1|__CRm__| CoRegTrans
/// |_Cond__|1_1_1_1|_____________Ignored_by_Processor______________| SWI
/// ```
fn arm_decode(i: u32) -> &'static str {
    const T: bool = true;
    const F: bool = false;

    // First, decode the the top-most non-condition bits
    match i.bit_range(26..28) {
        0b00 => {
            /* DataProcessing and friends */

            let result: Option<&str> = match (i.bit_range(23..26), i.bit_range(4..8)) {
                (0b000, 0b1001) => {
                    if 0b0 == i.ibit(22) {
                        Some("Multiply")
                    } else {
                        None
                    }
                }
                (0b001, 0b1001) => Some("MultiplyLong"),
                (0b010, 0b1001) => {
                    if 0b00 == i.bit_range(20..22) {
                        Some("SingleDataSwap")
                    } else {
                        None
                    }
                }
                (0b010, 0b0001) => {
                    if 0b010 == i.bit_range(20..23) {
                        Some("BranchExchange")
                    } else {
                        None
                    }
                }
                _ => None,
            };

            result.unwrap_or_else(|| {
                match (i.ibit(25), i.ibit(22), i.ibit(7), i.ibit(4)) {
                    (0, 0, 1, 1) => "HalfwordDataTransferRegOffset",
                    (0, 1, 1, 1) => "HalfwordDataTransferImmediateOffset",
                    _ => {
                        let set_cond_flags = i.bit(20);
                        // PSR Transfers are encoded as a subset of Data Processing,
                        // with S bit OFF and the encode opcode is one of TEQ,CMN,TST,CMN
                        let is_op_not_touching_rd = i.bit_range(21..25) & 0b1100 == 0b1000;
                        if !set_cond_flags && is_op_not_touching_rd {
                            if i.bit(21) {
                                // Since bit-16 is ignored and we can't know statically if this is a MoveToStatus or MoveToFlags
                                "MoveToStatus"
                            } else {
                                "MoveFromStatus"
                            }
                        } else {
                            "DataProcessing"
                        }
                    }
                }
            })
        }
        0b01 => {
            match (i.bit(25), i.bit(4)) {
                (_, F) | (F, T) => "SingleDataTransfer",
                (T, T) => "Undefined", /* Possible ARM11 but we don't implement these */
            }
        }
        0b10 => match i.bit(25) {
            F => "BlockDataTransfer",
            T => "BranchLink",
        },
        0b11 => {
            match (i.ibit(25), i.ibit(24), i.ibit(4)) {
                (0b0, _, _) => "Undefined", /* CoprocessorDataTransfer not implemented */
                (0b1, 0b0, 0b0) => "Undefined", /* CoprocessorDataOperation not implemented */
                (0b1, 0b0, 0b1) => "Undefined", /* CoprocessorRegisterTransfer not implemented */
                (0b1, 0b1, _) => "SoftwareInterrupt",
                _ => "Undefined",
            }
        }
        _ => unreachable!(),
    }
}

fn thumb_format_to_handler(thumb_fmt: &str) -> &'static str {
    match thumb_fmt {
        "MoveShiftedReg" => "exec_thumb_move_shifted_reg",
        "AddSub" => "exec_thumb_add_sub",
        "DataProcessImm" => "exec_thumb_data_process_imm",
        "AluOps" => "exec_thumb_alu_ops",
        "HiRegOpOrBranchExchange" => "exec_thumb_hi_reg_op_or_bx",
        "LdrPc" => "exec_thumb_ldr_pc",
        "LdrStrRegOffset" => "exec_thumb_ldr_str_reg_offset",
        "LdrStrSHB" => "exec_thumb_ldr_str_shb",
        "LdrStrImmOffset" => "exec_thumb_ldr_str_imm_offset",
        "LdrStrHalfWord" => "exec_thumb_ldr_str_halfword",
        "LdrStrSp" => "exec_thumb_ldr_str_sp",
        "LoadAddress" => "exec_thumb_load_address",
        "AddSp" => "exec_thumb_add_sp",
        "PushPop" => "exec_thumb_push_pop",
        "LdmStm" => "exec_thumb_ldm_stm",
        "BranchConditional" => "exec_thumb_branch_with_cond",
        "Swi" => "exec_thumb_swi",
        "Branch" => "exec_thumb_branch",
        "BranchLongWithLink" => "exec_thumb_branch_long_with_link",
        "Undefined" => "thumb_undefined",
        _ => unreachable!(),
    }
}

fn arm_format_to_handler(arm_fmt: &str) -> &'static str {
    match arm_fmt {
        "BranchExchange" => "exec_arm_bx",
        "BranchLink" => "exec_arm_b_bl",
        "DataProcessing" => "exec_arm_data_processing",
        "SoftwareInterrupt" => "exec_arm_swi",
        "SingleDataTransfer" => "exec_arm_ldr_str",
        "HalfwordDataTransferImmediateOffset" => "exec_arm_ldr_str_hs",
        "HalfwordDataTransferRegOffset" => "exec_arm_ldr_str_hs",
        "BlockDataTransfer" => "exec_arm_ldm_stm",
        "MoveFromStatus" => "exec_arm_mrs",
        "MoveToStatus" => "exec_arm_transfer_to_status",
        "MoveToFlags" => "exec_arm_transfer_to_status",
        "Multiply" => "exec_arm_mul_mla",
        "MultiplyLong" => "exec_arm_mull_mlal",
        "SingleDataSwap" => "exec_arm_swp",
        "Undefined" => "arm_undefined",
        _ => unreachable!(),
    }
}

fn generate_thumb_lut(file: &mut fs::File) -> Result<(), std::io::Error> {
    writeln!(
        file,
        "use super::thumb::ThumbFormat;

pub type ThumbInstructionHandler = fn(&mut Core, &mut SysBus, &ThumbInstruction) -> CpuAction;

pub struct ThumbInstructionInfo {{
    pub fmt: ThumbFormat,
    pub handler_fn: ThumbInstructionHandler
}}
"
    )?;

    writeln!(
        file,
        "pub const THUMB_LUT: [ThumbInstructionInfo; 1024] = ["
    )?;

    for i in 0..1024 {
        let thumb_fmt = thumb_decode(i << 6);
        let handler_name = thumb_format_to_handler(thumb_fmt);
        writeln!(
            file,
            "    /* {:#x} */ ThumbInstructionInfo {{ fmt: ThumbFormat::{}, handler_fn: Core::{} }},",
            i, thumb_fmt, handler_name
        )?;
    }

    writeln!(file, "];")?;

    Ok(())
}

fn generate_arm_lut(file: &mut fs::File) -> Result<(), std::io::Error> {
    writeln!(
        file,
        "use super::arm::ArmFormat;

pub type ArmInstructionHandler = fn(&mut Core, &mut SysBus, &ArmInstruction) -> CpuAction;

pub struct ArmInstructionInfo {{
    pub fmt: ArmFormat,
    pub handler_fn: ArmInstructionHandler
}}
"
    )?;

    writeln!(file, "pub const ARM_LUT: [ArmInstructionInfo; 4096] = [")?;
    for i in 0..4096 {
        let arm_fmt = arm_decode(((i & 0xff0) << 16) | ((i & 0x00f) << 4));
        let handler_name = arm_format_to_handler(arm_fmt);
        writeln!(
            file,
            "    /* {:#x} */ ArmInstructionInfo {{ fmt: ArmFormat::{}, handler_fn: Core::{} }},",
            i, arm_fmt, handler_name
        )?;
    }
    writeln!(file, "];")?;

    Ok(())
}

fn main() {
    let arm7tdmi_dispatch_table_enabled =
        env::var_os("CARGO_FEATURE_ARM7TDMI_DISPATCH_TABLE").is_some();

    if arm7tdmi_dispatch_table_enabled {
        // TODO - don't do this in the build script and use `const fn` instead when it becomes stable
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let thumb_lut_path = Path::new(&out_dir).join("thumb_lut.rs");
        let mut thumb_lut_file = fs::File::create(&thumb_lut_path).expect("failed to create file");
        generate_thumb_lut(&mut thumb_lut_file).expect("failed to generate thumb table");

        let arm_lut_path = Path::new(&out_dir).join("arm_lut.rs");
        let mut arm_lut_file = fs::File::create(&arm_lut_path).expect("failed to create file");
        generate_arm_lut(&mut arm_lut_file).expect("failed to generate arm table");

        println!("cargo:rerun-if-changed=build.rs");
    }
}
