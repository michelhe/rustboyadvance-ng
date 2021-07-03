use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

extern crate bit;
use bit::BitIndex;

// copied and slightly adjusted from src/core/arm7tdmi/thumb/mod.rs
fn thumb_decode(i: u16) -> (&'static str, String) {
    let offset5 = i.bit_range(6..11) as u8;
    let load = i.bit(11);
    if i & 0xf800 == 0x1800 {
        (
            "AddSub",
            format!(
                "exec_thumb_add_sub::<{SUB}, {IMM}, {RN}>",
                SUB = i.bit(9),
                IMM = i.bit(10),
                RN = i.bit_range(6..9) as usize
            ),
        )
    } else if i & 0xe000 == 0x0000 {
        (
            "MoveShiftedReg",
            format!(
                "exec_thumb_move_shifted_reg::<{BS_OP}, {IMM}>",
                BS_OP = i.bit_range(11..13) as u8,
                IMM = i.bit_range(6..11) as u8
            ),
        )
    } else if i & 0xe000 == 0x2000 {
        (
            "DataProcessImm",
            format!(
                "exec_thumb_data_process_imm::<{OP}, {RD}>",
                OP = i.bit_range(11..13) as u8,
                RD = i.bit_range(8..11)
            ),
        )
    } else if i & 0xfc00 == 0x4000 {
        (
            "AluOps",
            format!("exec_thumb_alu_ops::<{OP}>", OP = i.bit_range(6..10) as u16),
        )
    } else if i & 0xfc00 == 0x4400 {
        (
            "HiRegOpOrBranchExchange",
            format!(
                "exec_thumb_hi_reg_op_or_bx::<{OP}, {FLAG_H1}, {FLAG_H2}>",
                OP = i.bit_range(8..10) as u8,
                FLAG_H1 = i.bit(7),
                FLAG_H2 = i.bit(6),
            ),
        )
    } else if i & 0xf800 == 0x4800 {
        (
            "LdrPc",
            format!(
                "exec_thumb_ldr_pc::<{RD}>",
                RD = i.bit_range(8..11) as usize
            ),
        )
    } else if i & 0xf200 == 0x5000 {
        (
            "LdrStrRegOffset",
            format!(
                "exec_thumb_ldr_str_reg_offset::<{LOAD}, {RO}, {BYTE}>",
                LOAD = load,
                RO = i.bit_range(6..9) as usize,
                BYTE = i.bit(10),
            ),
        )
    } else if i & 0xf200 == 0x5200 {
        (
            "LdrStrSHB",
            format!(
                "exec_thumb_ldr_str_shb::<{RO}, {SIGN_EXTEND}, {HALFWORD}>",
                RO = i.bit_range(6..9) as usize,
                SIGN_EXTEND = i.bit(10),
                HALFWORD = i.bit(11),
            ),
        )
    } else if i & 0xe000 == 0x6000 {
        let is_transferring_bytes = i.bit(12);
        let offset = if is_transferring_bytes {
            offset5
        } else {
            (offset5 << 3) >> 1
        };
        (
            "LdrStrImmOffset",
            format!(
                "exec_thumb_ldr_str_imm_offset::<{LOAD}, {BYTE}, {OFFSET}>",
                LOAD = load,
                BYTE = is_transferring_bytes,
                OFFSET = offset
            ),
        )
    } else if i & 0xf000 == 0x8000 {
        (
            "LdrStrHalfWord",
            format!(
                "exec_thumb_ldr_str_halfword::<{LOAD}, {OFFSET}>",
                LOAD = load,
                OFFSET = (offset5 << 1) as i32
            ),
        )
    } else if i & 0xf000 == 0x9000 {
        (
            "LdrStrSp",
            format!(
                "exec_thumb_ldr_str_sp::<{LOAD}, {RD}>",
                LOAD = load,
                RD = i.bit_range(8..11)
            ),
        )
    } else if i & 0xf000 == 0xa000 {
        (
            "LoadAddress",
            format!(
                "exec_thumb_load_address::<{SP}, {RD}>",
                SP = i.bit(11),
                RD = i.bit_range(8..11)
            ),
        )
    } else if i & 0xff00 == 0xb000 {
        (
            "AddSp",
            format!("exec_thumb_add_sp::<{FLAG_S}>", FLAG_S = i.bit(7)),
        )
    } else if i & 0xf600 == 0xb400 {
        (
            "PushPop",
            format!(
                "exec_thumb_push_pop::<{POP}, {FLAG_R}>",
                POP = load,
                FLAG_R = i.bit(8)
            ),
        )
    } else if i & 0xf000 == 0xc000 {
        (
            "LdmStm",
            format!(
                "exec_thumb_ldm_stm::<{LOAD}, {RB}>",
                LOAD = load,
                RB = i.bit_range(8..11) as usize
            ),
        )
    } else if i & 0xff00 == 0xdf00 {
        ("Swi", String::from("exec_thumb_swi"))
    } else if i & 0xf000 == 0xd000 {
        (
            "BranchConditional",
            format!(
                "exec_thumb_branch_with_cond::<{COND}>",
                COND = i.bit_range(8..12) as u8
            ),
        )
    } else if i & 0xf800 == 0xe000 {
        ("Branch", String::from("exec_thumb_branch"))
    } else if i & 0xf000 == 0xf000 {
        (
            "BranchLongWithLink",
            format!(
                "exec_thumb_branch_long_with_link::<{FLAG_LOW_OFFSET}>",
                FLAG_LOW_OFFSET = i.bit(11),
            ),
        )
    } else {
        ("Undefined", String::from("thumb_undefined"))
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
fn arm_decode(i: u32) -> (&'static str, String) {
    const T: bool = true;
    const F: bool = false;

    // First, decode the the top-most non-condition bits
    match i.bit_range(26..28) {
        0b00 => {
            /* DataProcessing and friends */

            let result = match (i.bit_range(23..26), i.bit_range(4..8)) {
                (0b000, 0b1001) => {
                    if 0b0 == i.ibit(22) {
                        Some((
                            "Multiply",
                            format!(
                                "exec_arm_mul_mla::<{UPDATE_FLAGS}, {ACCUMULATE}>",
                                UPDATE_FLAGS = i.bit(20),
                                ACCUMULATE = i.bit(21),
                            ),
                        ))
                    } else {
                        None
                    }
                }
                (0b001, 0b1001) => Some((
                    "MultiplyLong",
                    format!(
                        "exec_arm_mull_mlal::<{UPDATE_FLAGS}, {ACCUMULATE}, {U_FLAG}>",
                        UPDATE_FLAGS = i.bit(20),
                        ACCUMULATE = i.bit(21),
                        U_FLAG = i.bit(22),
                    ),
                )),
                (0b010, 0b1001) => {
                    if 0b00 == i.bit_range(20..22) {
                        Some((
                            "SingleDataSwap",
                            format!("exec_arm_swp::<{BYTE}>", BYTE = i.bit(22)),
                        ))
                    } else {
                        None
                    }
                }
                (0b010, 0b0001) => {
                    if 0b010 == i.bit_range(20..23) {
                        Some(("BranchExchange", format!("exec_arm_bx")))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(result) = result {
                result
            } else {
                match (i.ibit(25), i.ibit(22), i.ibit(7), i.ibit(4)) {
                    (0, 0, 1, 1) => (
                        "HalfwordDataTransferRegOffset",
                        format!(
                            "exec_arm_ldr_str_hs_reg::<{HS}, {LOAD}, {WRITEBACK}, {PRE_INDEX}, {ADD}>",
                            HS = (i & 0b1100000) >> 5,
                            LOAD = i.bit(20),
                            WRITEBACK = i.bit(21),
                            ADD = i.bit(23),
                            PRE_INDEX = i.bit(24),
                        ),
                    ),
                    (0, 1, 1, 1) => (
                        "HalfwordDataTransferImmediateOffset",
                        format!(
                            "exec_arm_ldr_str_hs_imm::<{HS}, {LOAD}, {WRITEBACK}, {PRE_INDEX}, {ADD}>",
                            HS = (i & 0b1100000) >> 5,
                            LOAD = i.bit(20),
                            WRITEBACK = i.bit(21),
                            ADD = i.bit(23),
                            PRE_INDEX = i.bit(24)
                        ),
                    ),
                    _ => {
                        let set_cond_flags = i.bit(20);
                        // PSR Transfers are encoded as a subset of Data Processing,
                        // with S bit OFF and the encode opcode is one of TEQ,CMN,TST,CMN
                        let is_op_not_touching_rd = i.bit_range(21..25) & 0b1100 == 0b1000;
                        if !set_cond_flags && is_op_not_touching_rd {
                            if i.bit(21) {
                                ("MoveToStatus", format!("exec_arm_transfer_to_status::<{IMM}, {SPSR_FLAG}>",
                                    IMM = i.bit(25), SPSR_FLAG = i.bit(22)))
                            } else {
                                ("MoveFromStatus", format!("exec_arm_mrs::<{SPSR_FLAG}>", SPSR_FLAG = i.bit(22)))
                            }
                        } else {
                            ("DataProcessing", format!("exec_arm_data_processing::<{OP}, {IMM}, {SET_FLAGS}, {SHIFT_BY_REG}>",
                                OP=i.bit_range(21..25),
                                IMM=i.bit(25),
                                SET_FLAGS=i.bit(20),
                                SHIFT_BY_REG=i.bit(4)))
                        }
                    }
                }
            }
        }
        0b01 => {
            match (i.bit(25), i.bit(4)) {
                (_, F) | (F, T) => ("SingleDataTransfer", format!(
                    "exec_arm_ldr_str::<{LOAD}, {WRITEBACK}, {PRE_INDEX}, {BYTE}, {SHIFT}, {ADD}, {BS_OP}, {SHIFT_BY_REG}>",
                    LOAD = i.bit(20),
                    WRITEBACK = i.bit(21),
                    BYTE = i.bit(22),
                    ADD = i.bit(23),
                    PRE_INDEX = i.bit(24),
                    SHIFT = i.bit(25),
                    BS_OP = i.bit_range(5..7) as u8,
                    SHIFT_BY_REG = i.bit(4),
                )),
                (T, T) => ("Undefined", String::from("arm_undefined")), /* Possible ARM11 but we don't implement these */
            }
        }
        0b10 => match i.bit(25) {
            F => (
                "BlockDataTransfer",
                format!(
                    "exec_arm_ldm_stm::<{LOAD}, {WRITEBACK}, {FLAG_S}, {ADD}, {PRE_INDEX}>",
                    LOAD = i.bit(20),
                    WRITEBACK = i.bit(21),
                    FLAG_S = i.bit(22),
                    ADD = i.bit(23),
                    PRE_INDEX = i.bit(24),
                ),
            ),
            T => (
                "BranchLink",
                format!("exec_arm_b_bl::<{LINK}>", LINK = i.bit(24)),
            ),
        },
        0b11 => {
            match (i.ibit(25), i.ibit(24), i.ibit(4)) {
                (0b0, _, _) => ("Undefined", String::from("arm_undefined")), /* CoprocessorDataTransfer not implemented */
                (0b1, 0b0, 0b0) => ("Undefined", String::from("arm_undefined")), /* CoprocessorDataOperation not implemented */
                (0b1, 0b0, 0b1) => ("Undefined", String::from("arm_undefined")), /* CoprocessorRegisterTransfer not implemented */
                (0b1, 0b1, _) => ("SoftwareInterrupt", String::from("exec_arm_swi")),
                _ => ("Undefined", String::from("arm_undefined")),
            }
        }
        _ => unreachable!(),
    }
}

fn generate_thumb_lut(file: &mut fs::File) -> Result<(), std::io::Error> {
    writeln!(file, "impl<I: MemoryInterface> Core<I> {{")?;
    writeln!(
        file,
        "   pub const THUMB_LUT: [ThumbInstructionInfo<I>; 1024] = ["
    )?;

    for i in 0..1024 {
        let (thumb_fmt, handler_name) = thumb_decode(i << 6);
        writeln!(
            file,
            "       /* {:#x} */
        ThumbInstructionInfo {{
            handler_fn: Core::{},
            #[cfg(feature = \"debugger\")]
            fmt: ThumbFormat::{},
        }},",
            i, handler_name, thumb_fmt
        )?;
    }

    writeln!(file, "    ];")?;
    writeln!(file, "}}")?;

    Ok(())
}

fn generate_arm_lut(file: &mut fs::File) -> Result<(), std::io::Error> {
    writeln!(file, "impl<I: MemoryInterface> Core<I> {{")?;
    writeln!(
        file,
        "    pub const ARM_LUT: [ArmInstructionInfo<I>; 4096] = ["
    )?;
    for i in 0..4096 {
        let (arm_fmt, handler_name) = arm_decode(((i & 0xff0) << 16) | ((i & 0x00f) << 4));
        writeln!(
            file,
            "       /* {:#x} */
        ArmInstructionInfo {{
            handler_fn: Core::{},
            #[cfg(feature = \"debugger\")]
            fmt: ArmFormat::{},
        }} ,",
            i, handler_name, arm_fmt
        )?;
    }
    writeln!(file, "    ];")?;
    writeln!(file, "}}")?;

    Ok(())
}

fn main() {
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
