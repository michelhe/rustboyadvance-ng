//! ARM → native-code dynarec, backed by Cranelift.
//!
//! Early-stage scaffolding. The goal is to translate hot ARM data-processing
//! instructions into native machine code at block-record time and call the
//! compiled block from `Arm7tdmiCore::step_block` instead of replaying the
//! cached interpreter's decoded instruction stream.
//!
//! Current scope (MVP):
//!   - Cranelift JIT module wrapper (one per CPU instance).
//!   - Compile-and-invoke of a trivial function that manipulates the CPU's
//!     general-purpose register array. Proves the end-to-end path works
//!     without pulling in the full ARM decoder yet.
//!   - Unit test that runs the compiled function against a real gpr array
//!     and asserts the expected post-state.
//!
//! The next increments will:
//!   1. Define a `CpuCtx` layout struct matching the in-memory layout of
//!      `Arm7tdmiCore`'s gpr / pc / cpsr fields, and pass a pointer of that
//!      type through Cranelift.
//!   2. Per-instruction codegen for MOV/ADD/SUB/MVN (immediate and register
//!      forms) without memory access or flag-setting.
//!   3. Flag materialization on request (S-bit-set instructions).
//!   4. Memory access via a callback into the bus (trampoline).
//!   5. Condition-code check wrapper per instruction.
//!   6. Branch handling that either falls through to the next block lookup
//!      or re-enters the dispatcher.
//!
//! Until all of that is in place, `compile_block` returns `None` and the
//! cached interpreter keeps running. This module compiles (and its tests
//! pass) only when the `dynarec` cargo feature is enabled.

use cranelift::codegen::ir::immediates::Offset32;
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// Handle to a Cranelift JIT module. One per CPU instance; freed on CPU drop.
///
/// Wrapping the Cranelift state here keeps the module lifetime tied to the
/// CPU so generated code pages are reclaimed when the emulator tears down.
pub struct DynarecCompiler {
    module: JITModule,
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    next_id: u64,
}

impl DynarecCompiler {
    pub fn new() -> Self {
        let isa_builder = cranelift_native::builder()
            .expect("host architecture not supported by Cranelift");
        let flag_builder = settings::builder();
        let flags = settings::Flags::new(flag_builder);
        let isa = isa_builder
            .finish(flags)
            .expect("failed to build Cranelift ISA for host");

        let jit_builder =
            JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(jit_builder);
        let ctx = module.make_context();

        DynarecCompiler {
            module,
            builder_context: FunctionBuilderContext::new(),
            ctx,
            next_id: 0,
        }
    }

    /// Compile a sequence of supported ARM data-processing instructions
    /// into native code. Supported shapes:
    ///
    /// Writeback (S=0 or S=1):
    ///   - MOV Rd, op2    op=1101    writes Rd = op2
    ///   - MVN Rd, op2    op=1111    writes Rd = !op2
    ///   - ADD Rd, Rn,op2 op=0100    writes Rd = Rn+op2
    ///   - SUB Rd, Rn,op2 op=0010    writes Rd = Rn-op2
    ///
    /// Compare-only (S=1 mandatory, no writeback):
    ///   - CMP Rn, op2    op=1010    flags from Rn-op2
    ///   - CMN Rn, op2    op=1011    flags from Rn+op2
    ///   - TST Rn, op2    op=1000    flags from Rn&op2
    ///   - TEQ Rn, op2    op=1001    flags from Rn^op2
    ///
    /// operand2 is either an immediate (I=1) or a register with no shift.
    /// All 14 ARM condition codes (EQ/NE/HS/LO/MI/PL/VS/VC/HI/LS/GE/LT/GT/LE/AL)
    /// are supported via runtime CPSR.NZCV check.
    ///
    /// For S=1 / compare-only instructions, the dynarec computes and writes
    /// back N, Z, C, V to the caller-owned CPSR word at `cpsr_ptr`.
    ///
    /// Returns `None` if any opcode is not one of the supported shapes.
    pub fn try_compile_imm_block(
        &mut self,
        opcodes: &[u32],
    ) -> Option<extern "C" fn(*mut u32, *mut u32)> {
        for &insn in opcodes {
            if Self::decode_supported_dp(insn).is_none() {
                return None;
            }
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));   // gpr_ptr
        sig.params.push(AbiParam::new(ptr_type));   // cpsr_ptr

        self.next_id += 1;
        let name = format!("dynarec_imm_block_{}", self.next_id);
        let func_id = self
            .module
            .declare_function(&name, Linkage::Local, &sig)
            .expect("declare_function failed");
        self.ctx.func.signature = sig;

        {
            let mut builder =
                FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            builder.seal_block(entry);

            let gpr_ptr = builder.block_params(entry)[0];
            let cpsr_ptr = builder.block_params(entry)[1];

            // Load CPSR into a mutable variable at function entry. Cond
            // checks read from it; S-bit / compare ops write back to it.
            // Storing once at function exit.
            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder
                    .ins()
                    .load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            for &insn in opcodes {
                let dec = Self::decode_supported_dp(insn).expect("pre-validated");
                emit_conditional_instr(&mut builder, gpr_ptr, cpsr_var, dec);
            }

            // Flush CPSR back out.
            let cpsr_final = builder.use_var(cpsr_var);
            builder
                .ins()
                .store(MemFlags::trusted(), cpsr_final, cpsr_ptr, 0);

            builder.ins().return_(&[]);
            builder.finalize();
        }

        self.module
            .define_function(func_id, &mut self.ctx)
            .expect("define_function failed");
        self.module.clear_context(&mut self.ctx);
        self.module
            .finalize_definitions()
            .expect("finalize_definitions failed");

        let code = self.module.get_finalized_function(func_id);
        // SAFETY: declared signature matches the transmute target.
        Some(unsafe {
            std::mem::transmute::<*const u8, extern "C" fn(*mut u32, *mut u32)>(code)
        })
    }

    /// Classify an ARM opcode as one of the supported data-processing shapes
    /// (immediate or register with no shift). Returns the decoded fields
    /// needed for codegen, or None for any unsupported encoding.
    fn decode_supported_dp(insn: u32) -> Option<DecodedDp> {
        let cond_bits = (insn >> 28) & 0xf;
        // NV (0xF) is reserved / invalid in ARMv4; skip.
        if cond_bits == 0xF {
            return None;
        }
        let cond = ArmCond::from_bits(cond_bits as u8);
        let class = (insn >> 20) & 0xff;
        // Data-processing bits [27:26] = 0b00.
        // Immediate form has bit 5 (of class) = 1 (= insn bit 25 = I).
        // Register form has I = 0.
        if (class & 0b1100_0000) != 0b0000_0000 {
            return None;
        }
        let i_bit = (class >> 5) & 1;
        let s_bit = (class & 1) != 0;
        let op_raw = (class >> 1) & 0xf;
        let op = match op_raw {
            0b1101 => DpOp::Mov,
            0b1111 => DpOp::Mvn,
            0b0100 => DpOp::Add,
            0b0010 => DpOp::Sub,
            0b1010 if s_bit => DpOp::Cmp,
            0b1011 if s_bit => DpOp::Cmn,
            0b1000 if s_bit => DpOp::Tst,
            0b1001 if s_bit => DpOp::Teq,
            _ => return None,
        };
        let rn = ((insn >> 16) & 0xf) as i32;
        let rd = ((insn >> 12) & 0xf) as i32;
        if !(0..15).contains(&rd) || !(0..15).contains(&rn) {
            return None;
        }

        let operand2 = if i_bit == 1 {
            // Immediate form: 8-bit value rotated right by 2 × rot4.
            let imm8 = insn & 0xff;
            let rot = ((insn >> 8) & 0xf) * 2;
            Operand2::Imm(imm8.rotate_right(rot))
        } else {
            // Register form: require no shift (shift_imm=0, shift_type=00)
            // and bit 4 = 0 (distinguishes from shift-by-register, which
            // has different timing and pipeline semantics).
            let shift_field = (insn >> 4) & 0xff;
            if shift_field != 0 {
                return None;
            }
            let rm = (insn & 0xf) as i32;
            if !(0..15).contains(&rm) {
                return None;
            }
            Operand2::Reg(rm)
        };

        Some(DecodedDp { cond, op, rd, rn, operand2, s: s_bit })
    }

    /// Compile a stub function that reads `gpr[1]` and writes it into
    /// `gpr[0]`. Used as an end-to-end smoke test for the JIT pipeline —
    /// proves codegen, define, and finalize all round-trip before we invest
    /// in the real ARM decoder→IR lowering.
    ///
    /// Returns an `extern "C"` fn pointer; the closure-wrapper is kept alive
    /// by the JITModule owned by this compiler, so the returned pointer is
    /// valid until `DynarecCompiler` drops.
    pub fn compile_mov_r0_r1_stub(&mut self) -> extern "C" fn(*mut u32) {
        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));

        self.next_id += 1;
        let name = format!("dynarec_stub_{}", self.next_id);
        let func_id = self
            .module
            .declare_function(&name, Linkage::Local, &sig)
            .expect("declare_function failed");
        self.ctx.func.signature = sig;

        {
            let mut builder =
                FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            builder.seal_block(entry);

            let gpr_ptr = builder.block_params(entry)[0];
            // Load gpr[1] (4-byte offset into the u32 array).
            let r1 =
                builder
                    .ins()
                    .load(types::I32, MemFlags::trusted(), gpr_ptr, 4);
            // Store into gpr[0] (offset 0).
            builder
                .ins()
                .store(MemFlags::trusted(), r1, gpr_ptr, 0);
            builder.ins().return_(&[]);
            builder.finalize();
        }

        self.module
            .define_function(func_id, &mut self.ctx)
            .expect("define_function failed");
        self.module.clear_context(&mut self.ctx);
        self.module
            .finalize_definitions()
            .expect("finalize_definitions failed");

        let code = self.module.get_finalized_function(func_id);
        // SAFETY: the pointer Cranelift returns is a valid executable
        // function matching the signature we declared. We've pinned its
        // lifetime to `self` by keeping the JITModule alive here.
        unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut u32)>(code) }
    }
}

impl Default for DynarecCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Subset of ARM data-processing opcodes the dynarec can currently emit.
/// Cmp/Cmn/Tst/Teq are compare-only (no Rd writeback, S=1 mandatory).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DpOp {
    Mov,
    Mvn,
    Add,
    Sub,
    Cmp,
    Cmn,
    Tst,
    Teq,
}

impl DpOp {
    fn is_compare_only(self) -> bool {
        matches!(self, DpOp::Cmp | DpOp::Cmn | DpOp::Tst | DpOp::Teq)
    }
}

/// The second operand of a data-processing instruction.
#[derive(Clone, Copy, Debug)]
enum Operand2 {
    Imm(u32),
    Reg(i32),
}

#[derive(Clone, Copy, Debug)]
struct DecodedDp {
    cond: ArmCond,
    op: DpOp,
    rd: i32,
    rn: i32,
    operand2: Operand2,
    /// Update CPSR.NZCV after executing this instruction. Compare-only ops
    /// (CMP/CMN/TST/TEQ) always have s=true.
    s: bool,
}

/// ARM condition-code mnemonics. Evaluated against the NZCV bits of CPSR
/// before each instruction body runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArmCond {
    Eq, Ne, Hs, Lo, Mi, Pl, Vs, Vc,
    Hi, Ls, Ge, Lt, Gt, Le, Al,
}

impl ArmCond {
    fn from_bits(bits: u8) -> Self {
        use ArmCond::*;
        match bits & 0xf {
            0x0 => Eq, 0x1 => Ne, 0x2 => Hs, 0x3 => Lo,
            0x4 => Mi, 0x5 => Pl, 0x6 => Vs, 0x7 => Vc,
            0x8 => Hi, 0x9 => Ls, 0xA => Ge, 0xB => Lt,
            0xC => Gt, 0xD => Le, 0xE => Al,
            _ => Al, // 0xF handled upstream
        }
    }
}

/// Emit Cranelift IR that evaluates an ARM condition code against the CPSR
/// variable and returns a `bool` (i8 in IR) that's true when the condition
/// passes.
fn emit_cond_check(
    builder: &mut FunctionBuilder,
    cpsr_var: Variable,
    cond: ArmCond,
) -> Value {
    let cpsr = builder.use_var(cpsr_var);
    // Flag bit positions in CPSR: N=31, Z=30, C=29, V=28.
    let one = builder.ins().iconst(types::I32, 1);
    let n = {
        let shifted = builder.ins().ushr_imm(cpsr, 31);
        builder.ins().band(shifted, one)
    };
    let z = {
        let shifted = builder.ins().ushr_imm(cpsr, 30);
        builder.ins().band(shifted, one)
    };
    let c = {
        let shifted = builder.ins().ushr_imm(cpsr, 29);
        builder.ins().band(shifted, one)
    };
    let v = {
        let shifted = builder.ins().ushr_imm(cpsr, 28);
        builder.ins().band(shifted, one)
    };
    let zero = builder.ins().iconst(types::I32, 0);

    let true_val = builder.ins().iconst(types::I8, 1);

    match cond {
        ArmCond::Al => true_val,
        ArmCond::Eq => builder.ins().icmp(IntCC::NotEqual, z, zero),
        ArmCond::Ne => builder.ins().icmp(IntCC::Equal, z, zero),
        ArmCond::Hs => builder.ins().icmp(IntCC::NotEqual, c, zero),
        ArmCond::Lo => builder.ins().icmp(IntCC::Equal, c, zero),
        ArmCond::Mi => builder.ins().icmp(IntCC::NotEqual, n, zero),
        ArmCond::Pl => builder.ins().icmp(IntCC::Equal, n, zero),
        ArmCond::Vs => builder.ins().icmp(IntCC::NotEqual, v, zero),
        ArmCond::Vc => builder.ins().icmp(IntCC::Equal, v, zero),
        ArmCond::Hi => {
            // C=1 && Z=0
            let c_set = builder.ins().icmp(IntCC::NotEqual, c, zero);
            let z_clear = builder.ins().icmp(IntCC::Equal, z, zero);
            builder.ins().band(c_set, z_clear)
        }
        ArmCond::Ls => {
            // C=0 || Z=1
            let c_clear = builder.ins().icmp(IntCC::Equal, c, zero);
            let z_set = builder.ins().icmp(IntCC::NotEqual, z, zero);
            builder.ins().bor(c_clear, z_set)
        }
        ArmCond::Ge => builder.ins().icmp(IntCC::Equal, n, v),
        ArmCond::Lt => builder.ins().icmp(IntCC::NotEqual, n, v),
        ArmCond::Gt => {
            // Z=0 && N==V
            let z_clear = builder.ins().icmp(IntCC::Equal, z, zero);
            let nv_eq = builder.ins().icmp(IntCC::Equal, n, v);
            builder.ins().band(z_clear, nv_eq)
        }
        ArmCond::Le => {
            // Z=1 || N!=V
            let z_set = builder.ins().icmp(IntCC::NotEqual, z, zero);
            let nv_ne = builder.ins().icmp(IntCC::NotEqual, n, v);
            builder.ins().bor(z_set, nv_ne)
        }
    }
}

/// Emit a conditionally-executed data-processing instruction. For AL cond
/// this is the same as emit_data_processing_imm; for any other cond we
/// wrap the body in a brif/skip pattern.
fn emit_conditional_instr(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedDp,
) {
    if dec.cond == ArmCond::Al {
        emit_data_processing_imm(builder, gpr_ptr, cpsr_var, dec);
        return;
    }

    let cond_result = emit_cond_check(builder, cpsr_var, dec.cond);
    let body = builder.create_block();
    let merge = builder.create_block();
    builder.ins().brif(cond_result, body, &[], merge, &[]);
    builder.switch_to_block(body);
    builder.seal_block(body);
    emit_data_processing_imm(builder, gpr_ptr, cpsr_var, dec);
    builder.ins().jump(merge, &[]);
    builder.switch_to_block(merge);
    builder.seal_block(merge);
}

/// Emit Cranelift IR for a single decoded data-processing instruction.
/// - Loads operand2 and (if applicable) rn from the gpr array.
/// - Computes the result.
/// - Writes the result back to gpr[rd] unless the op is compare-only.
/// - If S=1, updates the caller's cpsr variable with new NZCV flags.
fn emit_data_processing_imm(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedDp,
) {
    let op2 = match dec.operand2 {
        Operand2::Imm(v) => builder.ins().iconst(types::I32, v as i64),
        Operand2::Reg(rm) => builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            gpr_ptr,
            Offset32::new(rm * 4),
        ),
    };

    // rn is only meaningful for ops other than MOV/MVN; loading it
    // unconditionally is safe (register file access is cheap, LLVM will
    // remove dead loads) but not worth the cycle if we can skip.
    let need_rn = !matches!(dec.op, DpOp::Mov | DpOp::Mvn);
    let rn = if need_rn {
        builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            gpr_ptr,
            Offset32::new(dec.rn * 4),
        )
    } else {
        // Placeholder — unused.
        builder.ins().iconst(types::I32, 0)
    };

    let result = match dec.op {
        DpOp::Mov => op2,
        DpOp::Mvn => builder.ins().bnot(op2),
        DpOp::Add | DpOp::Cmn => builder.ins().iadd(rn, op2),
        DpOp::Sub | DpOp::Cmp => builder.ins().isub(rn, op2),
        DpOp::Tst => builder.ins().band(rn, op2),
        DpOp::Teq => builder.ins().bxor(rn, op2),
    };

    // Writeback (unless compare-only).
    if !dec.op.is_compare_only() {
        builder.ins().store(
            MemFlags::trusted(),
            result,
            gpr_ptr,
            Offset32::new(dec.rd * 4),
        );
    }

    // Flag update.
    if dec.s {
        let new_cpsr = emit_flag_update(builder, cpsr_var, dec.op, rn, op2, result);
        builder.def_var(cpsr_var, new_cpsr);
    }
}

/// Produce a new CPSR value with N/Z/C/V updated per the ARM rules for the
/// given opcode.
fn emit_flag_update(
    builder: &mut FunctionBuilder,
    cpsr_var: Variable,
    op: DpOp,
    rn: Value,
    op2: Value,
    result: Value,
) -> Value {
    let zero = builder.ins().iconst(types::I32, 0);
    let one = builder.ins().iconst(types::I32, 1);

    // N = result >> 31.
    let n = builder.ins().ushr_imm(result, 31);

    // Z = (result == 0) ? 1 : 0.
    let z_bool = builder.ins().icmp(IntCC::Equal, result, zero);
    let z = builder.ins().uextend(types::I32, z_bool);

    // C and V depend on op:
    //   ADD/CMN: C = unsigned carry-out; V = signed overflow.
    //   SUB/CMP: C = NOT borrow; V = signed overflow.
    //   Logical (TST/TEQ/MOV/MVN with S=1): C unchanged (we preserve
    //     the previous C bit), V unchanged.
    let (c, v) = match op {
        DpOp::Add | DpOp::Cmn => {
            // C: result < rn (unsigned) → carry out
            let c_bool = builder.ins().icmp(IntCC::UnsignedLessThan, result, rn);
            let c = builder.ins().uextend(types::I32, c_bool);
            // V: (~(rn ^ op2) & (rn ^ result)) >> 31
            let xor_ab = builder.ins().bxor(rn, op2);
            let nxor_ab = builder.ins().bnot(xor_ab);
            let xor_ar = builder.ins().bxor(rn, result);
            let v_bits = builder.ins().band(nxor_ab, xor_ar);
            let v = builder.ins().ushr_imm(v_bits, 31);
            (c, v)
        }
        DpOp::Sub | DpOp::Cmp => {
            // C: rn >= op2 (unsigned) → not-borrow
            let c_bool = builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, rn, op2);
            let c = builder.ins().uextend(types::I32, c_bool);
            // V: ((rn ^ op2) & (rn ^ result)) >> 31
            let xor_ab = builder.ins().bxor(rn, op2);
            let xor_ar = builder.ins().bxor(rn, result);
            let v_bits = builder.ins().band(xor_ab, xor_ar);
            let v = builder.ins().ushr_imm(v_bits, 31);
            (c, v)
        }
        DpOp::Mov | DpOp::Mvn | DpOp::Tst | DpOp::Teq => {
            // Logical ops preserve C and V (no shifter carry is computed
            // for the simple imm/reg-no-shift shapes we support yet).
            let cpsr = builder.use_var(cpsr_var);
            let c = {
                let shifted = builder.ins().ushr_imm(cpsr, 29);
                builder.ins().band(shifted, one)
            };
            let v = {
                let shifted = builder.ins().ushr_imm(cpsr, 28);
                builder.ins().band(shifted, one)
            };
            (c, v)
        }
    };

    // Merge new NZCV into CPSR (bits 31/30/29/28), preserving the others.
    let cpsr = builder.use_var(cpsr_var);
    let mask = builder.ins().iconst(types::I32, 0x0fff_ffff); // clear top 4 bits
    let cleared = builder.ins().band(cpsr, mask);
    let n_shifted = builder.ins().ishl_imm(n, 31);
    let z_shifted = builder.ins().ishl_imm(z, 30);
    let c_shifted = builder.ins().ishl_imm(c, 29);
    let v_shifted = builder.ins().ishl_imm(v, 28);
    let nz = builder.ins().bor(n_shifted, z_shifted);
    let cv = builder.ins().bor(c_shifted, v_shifted);
    let flags = builder.ins().bor(nz, cv);
    builder.ins().bor(cleared, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Differential-execution test: run the same ARM opcode sequence through
    /// the scalar interpreter and through the dynarec, assert the resulting
    /// register file matches. Catches any divergence introduced when we
    /// add new opcode shapes.
    fn differential(opcodes: &[u32], initial_gpr: [u32; 15]) {
        use crate::cpu::{Arm7tdmiCore, CpuAction};
        use crate::memory::MemoryAccess;
        use rustboyadvance_utils::Shared;

        // Build a SimpleMemory program from the opcodes, starting at PC 0.
        let mut program = Vec::with_capacity(opcodes.len() * 4);
        for op in opcodes {
            program.extend_from_slice(&op.to_le_bytes());
        }

        // --- Interpreter run ---
        let mut mem = crate::SimpleMemory::new(1024);
        mem.load_program(&program);
        let mem_shared = Shared::new(mem);
        let mut cpu = Arm7tdmiCore::new(mem_shared);
        cpu.gpr = initial_gpr;
        // Drive each opcode through step_arm_exec directly — avoids the
        // pipeline dance, which isn't the point of this test.
        for &op in opcodes {
            // The handler reads operands from gpr; ignore the returned
            // CpuAction since none of our supported shapes flush the
            // pipeline.
            let _: CpuAction = {
                let hash = (((op >> 16) & 0xff0) | ((op >> 4) & 0xf)) as usize;
                let arm_info = &Arm7tdmiCore::<crate::SimpleMemory>::ARM_LUT[hash];
                (arm_info.handler_fn)(&mut cpu, op)
            };
            // We intentionally skip next_fetch_access updates — SimpleMemory
            // doesn't do timing anyway.
            let _ = MemoryAccess::NonSeq;
        }
        let interp_gpr = cpu.gpr;

        // --- Dynarec run ---
        let mut compiler = DynarecCompiler::new();
        let func = compiler
            .try_compile_imm_block(opcodes)
            .expect("dynarec should support these opcodes");
        let mut dyn_gpr = initial_gpr;
        // AL condition on every instr → cpsr value doesn't matter. For S=1
        // ops it will get written back to this buffer.
        let mut dyn_cpsr = 0u32;
        func(dyn_gpr.as_mut_ptr(), &mut dyn_cpsr);

        assert_eq!(
            dyn_gpr, interp_gpr,
            "dynarec and interpreter diverged on block {:x?}",
            opcodes
        );
    }

    /// Same idea as `differential` but also compares the CPSR NZCV bits
    /// after execution, so we catch flag-computation bugs in the S-bit /
    /// compare-only paths.
    fn differential_with_flags(opcodes: &[u32], initial_gpr: [u32; 15], initial_cpsr: u32) {
        use crate::cpu::Arm7tdmiCore;
        use rustboyadvance_utils::Shared;

        let mut program = Vec::with_capacity(opcodes.len() * 4);
        for op in opcodes {
            program.extend_from_slice(&op.to_le_bytes());
        }

        // --- Interpreter run ---
        let mut mem = crate::SimpleMemory::new(1024);
        mem.load_program(&program);
        let mem_shared = Shared::new(mem);
        let mut cpu = Arm7tdmiCore::new(mem_shared);
        cpu.gpr = initial_gpr;
        // Overwrite CPSR with the test's desired initial flag state.
        cpu.cpsr = crate::psr::RegPSR::new(initial_cpsr);
        for &op in opcodes {
            let hash = (((op >> 16) & 0xff0) | ((op >> 4) & 0xf)) as usize;
            let arm_info = &Arm7tdmiCore::<crate::SimpleMemory>::ARM_LUT[hash];
            let _ = (arm_info.handler_fn)(&mut cpu, op);
        }
        let interp_gpr = cpu.gpr;
        let interp_cpsr = cpu.cpsr.get() & 0xF000_0000; // compare NZCV only

        // --- Dynarec run ---
        let mut compiler = DynarecCompiler::new();
        let func = compiler
            .try_compile_imm_block(opcodes)
            .expect("dynarec should support these opcodes");
        let mut dyn_gpr = initial_gpr;
        let mut dyn_cpsr = initial_cpsr;
        func(dyn_gpr.as_mut_ptr(), &mut dyn_cpsr);
        let dyn_cpsr_flags = dyn_cpsr & 0xF000_0000;

        assert_eq!(
            dyn_gpr, interp_gpr,
            "gpr diverged on {:x?}\ninterp={:?}\ndynrec={:?}",
            opcodes, interp_gpr, dyn_gpr
        );
        assert_eq!(
            dyn_cpsr_flags, interp_cpsr,
            "NZCV diverged on {:x?}: interp={:#010x} dynarec={:#010x}",
            opcodes, interp_cpsr, dyn_cpsr_flags
        );
    }

    #[test]
    fn differential_cmp_various() {
        // CMP R0, #5 with R0 = {3, 5, 10, 0, 0xFFFFFFFF, 0x80000000}
        let block = [0xE350_0005u32];
        for &r0 in &[3u32, 5, 10, 0, 0xFFFFFFFFu32, 0x80000000u32] {
            let mut gpr = [0u32; 15];
            gpr[0] = r0;
            differential_with_flags(&block, gpr, 0);
        }
    }

    #[test]
    fn differential_adds_overflow_cases() {
        // ADDS R0, R0, #1 at interesting boundary values.
        let block = [0xE290_0001u32];
        for &r0 in &[
            0u32,
            1,
            0x7FFF_FFFF, // signed overflow
            0xFFFF_FFFF, // unsigned overflow/wrap
            0x8000_0000,
            0xFFFF_FFFE,
        ] {
            let mut gpr = [0u32; 15];
            gpr[0] = r0;
            differential_with_flags(&block, gpr, 0);
        }
    }

    #[test]
    fn differential_subs_borrow_cases() {
        // SUBS R0, R0, #1
        let block = [0xE250_0001u32];
        for &r0 in &[
            0u32,
            1,
            0x8000_0000, // signed overflow via subtract
            0x7FFF_FFFF,
            0xFFFF_FFFF,
        ] {
            let mut gpr = [0u32; 15];
            gpr[0] = r0;
            differential_with_flags(&block, gpr, 0);
        }
    }

    #[test]
    fn differential_tst_teq() {
        // TST R0, #0xFF; TEQ R0, #0xFF
        for opcode in &[0xE310_00FFu32, 0xE330_00FFu32] {
            for &r0 in &[0u32, 1, 0xFF, 0x100, 0xFFFF_FFFF, 0x8000_0000] {
                let mut gpr = [0u32; 15];
                gpr[0] = r0;
                differential_with_flags(&[*opcode], gpr, 0);
            }
        }
    }

    #[test]
    fn differential_mov_imm_sequence() {
        // Three MOVs with different Rd, different immediates.
        let block = [0xE3A0_0001u32, 0xE3A0_1002u32, 0xE3A0_20FFu32];
        differential(&block, [0; 15]);
    }

    #[test]
    fn differential_arithmetic_chain() {
        // MOV R0, #10; ADD R1, R0, #5; SUB R2, R1, #3; MVN R3, #0
        let block = [
            0xE3A0_000Au32,
            0xE280_1005u32,
            0xE241_2003u32,
            0xE3E0_3000u32,
        ];
        differential(&block, [0; 15]);
    }

    #[test]
    fn differential_register_form_chain() {
        // MOV R0, #10; MOV R1, #20; ADD R2, R0, R1; SUB R3, R1, R0
        let block = [
            0xE3A0_000Au32,
            0xE3A0_1014u32,
            0xE080_2001u32,
            0xE041_3000u32,
        ];
        differential(&block, [0; 15]);
    }

    #[test]
    fn differential_nonzero_initial_state() {
        // ADD R2, R0, R1 with gpr[0]=100, gpr[1]=50 → gpr[2]=150
        let block = [0xE080_2001u32];
        let mut initial = [0u32; 15];
        initial[0] = 100;
        initial[1] = 50;
        differential(&block, initial);
    }


    #[test]
    fn compile_and_run_mov_stub() {
        let mut compiler = DynarecCompiler::new();
        let func = compiler.compile_mov_r0_r1_stub();

        let mut gpr = [0u32; 15];
        gpr[1] = 0xDEAD_BEEF;
        func(gpr.as_mut_ptr());
        assert_eq!(gpr[0], 0xDEAD_BEEF);
        // Verify we didn't scribble anywhere else.
        assert_eq!(gpr[2], 0);
        assert_eq!(gpr[14], 0);
    }

    #[test]
    fn compile_real_mov_imm_sequence() {
        let mut compiler = DynarecCompiler::new();
        // Encode three MOV immediates:
        //   E3A0_0001  MOV R0, #1
        //   E3A0_1002  MOV R1, #2
        //   E3A0_20FF  MOV R2, #255
        let block = [0xE3A0_0001u32, 0xE3A0_1002u32, 0xE3A0_20FFu32];
        let func = compiler.try_compile_imm_block(&block).expect("should compile");

        let mut gpr = [0u32; 15];
        // Pre-poison to prove the writes actually happen.
        for v in gpr.iter_mut() {
            *v = 0xFFFF_FFFF;
        }
        { let mut cpsr = 0u32; func(gpr.as_mut_ptr(), &mut cpsr); }
        assert_eq!(gpr[0], 1);
        assert_eq!(gpr[1], 2);
        assert_eq!(gpr[2], 255);
        // Unchanged registers keep their poison.
        assert_eq!(gpr[3], 0xFFFF_FFFF);
    }

    #[test]
    fn compile_register_form_add() {
        let mut compiler = DynarecCompiler::new();
        // Block:
        //   MOV R0, #10         E3A0_000A
        //   MOV R1, #20         E3A0_1014
        //   ADD R2, R0, R1      E080_2001   (register-form, no shift)
        //   SUB R3, R1, R0      E041_3000   (register-form, no shift)
        let block = [
            0xE3A0_000Au32,
            0xE3A0_1014u32,
            0xE080_2001u32,
            0xE041_3000u32,
        ];
        let func = compiler
            .try_compile_imm_block(&block)
            .expect("should compile");

        let mut gpr = [0u32; 15];
        { let mut cpsr = 0u32; func(gpr.as_mut_ptr(), &mut cpsr); }
        assert_eq!(gpr[0], 10);
        assert_eq!(gpr[1], 20);
        assert_eq!(gpr[2], 30);
        assert_eq!(gpr[3], 10);
    }

    #[test]
    fn reject_register_form_with_shift() {
        let mut compiler = DynarecCompiler::new();
        // ADD R0, R1, R2, LSL #1 — shift nonzero, should not compile yet.
        let block = [0xE081_0082u32];
        assert!(compiler.try_compile_imm_block(&block).is_none());
    }

    #[test]
    fn compile_add_sub_mvn_mixed() {
        let mut compiler = DynarecCompiler::new();
        // Block:
        //   MOV R0, #10         E3A0_000A
        //   ADD R1, R0, #5      E280_1005
        //   SUB R2, R1, #3      E241_2003
        //   MVN R3, #0          E3E0_3000   (result = 0xFFFF_FFFF)
        let block = [
            0xE3A0_000Au32,
            0xE280_1005u32,
            0xE241_2003u32,
            0xE3E0_3000u32,
        ];
        let func = compiler.try_compile_imm_block(&block).expect("should compile");

        let mut gpr = [0u32; 15];
        { let mut cpsr = 0u32; func(gpr.as_mut_ptr(), &mut cpsr); }
        assert_eq!(gpr[0], 10);
        assert_eq!(gpr[1], 15);
        assert_eq!(gpr[2], 12);
        assert_eq!(gpr[3], 0xFFFF_FFFF);
    }

    #[test]
    fn reject_unsupported_opcode_returns_none() {
        let mut compiler = DynarecCompiler::new();
        // An LDR opcode (not data-processing-immediate). Should be rejected.
        let block = [0xE590_0000u32];
        assert!(compiler.try_compile_imm_block(&block).is_none());

        // NV (0xF) condition is reserved in ARMv4 and should be rejected.
        let block = [0xF3A0_0001u32];
        assert!(compiler.try_compile_imm_block(&block).is_none());
    }

    #[test]
    fn cmp_sets_zero_flag() {
        let mut compiler = DynarecCompiler::new();
        // CMP R0, #5    E350_0005
        let block = [0xE350_0005u32];
        let func = compiler.try_compile_imm_block(&block).expect("CMP supported");

        // R0 == 5 → Z=1, N=0, C=1 (no borrow), V=0
        let mut gpr = [0u32; 15];
        gpr[0] = 5;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!((cpsr >> 30) & 1, 1, "Z should be set for equal");
        assert_eq!((cpsr >> 31) & 1, 0, "N should be clear");
        assert_eq!((cpsr >> 29) & 1, 1, "C should be set (no borrow)");
        assert_eq!((cpsr >> 28) & 1, 0, "V should be clear");
        assert_eq!(gpr[0], 5, "CMP must not write Rd");
    }

    #[test]
    fn cmp_sets_negative_flag() {
        let mut compiler = DynarecCompiler::new();
        // CMP R0, #5   R0=3 → result = -2, N=1, Z=0, C=0 (borrow)
        let block = [0xE350_0005u32];
        let func = compiler.try_compile_imm_block(&block).expect("CMP supported");

        let mut gpr = [0u32; 15];
        gpr[0] = 3;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!((cpsr >> 31) & 1, 1, "N should be set");
        assert_eq!((cpsr >> 30) & 1, 0, "Z should be clear");
        assert_eq!((cpsr >> 29) & 1, 0, "C should be clear (borrow)");
    }

    #[test]
    fn tst_sets_zero_on_no_overlap() {
        let mut compiler = DynarecCompiler::new();
        // TST R0, #0x0F   E310_000F
        let block = [0xE310_000Fu32];
        let func = compiler.try_compile_imm_block(&block).expect("TST supported");

        // R0 = 0xF0 → R0 AND 0x0F = 0 → Z=1
        let mut gpr = [0u32; 15];
        gpr[0] = 0xF0;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!((cpsr >> 30) & 1, 1);
        assert_eq!(gpr[0], 0xF0, "TST must not write Rd");

        // R0 = 0xF1 → AND = 1 → Z=0, N=0
        let mut gpr = [0u32; 15];
        gpr[0] = 0xF1;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!((cpsr >> 30) & 1, 0);
        assert_eq!((cpsr >> 31) & 1, 0);
    }

    #[test]
    fn adds_sets_all_flags_correctly() {
        let mut compiler = DynarecCompiler::new();
        // ADDS R0, R0, #1   E290_0001
        let block = [0xE290_0001u32];
        let func = compiler.try_compile_imm_block(&block).expect("ADDS supported");

        // 0xFFFF_FFFF + 1 = 0 → Z=1, C=1 (carry), N=0, V=0
        let mut gpr = [0u32; 15];
        gpr[0] = 0xFFFF_FFFF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0);
        assert_eq!((cpsr >> 30) & 1, 1, "Z");
        assert_eq!((cpsr >> 29) & 1, 1, "C");
        assert_eq!((cpsr >> 31) & 1, 0, "N");
        assert_eq!((cpsr >> 28) & 1, 0, "V");

        // 0x7FFF_FFFF + 1 = 0x8000_0000 → N=1, V=1 (signed overflow)
        let mut gpr = [0u32; 15];
        gpr[0] = 0x7FFF_FFFF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0x8000_0000);
        assert_eq!((cpsr >> 31) & 1, 1, "N");
        assert_eq!((cpsr >> 28) & 1, 1, "V");
        assert_eq!((cpsr >> 29) & 1, 0, "C");
    }

    #[test]
    fn moveq_with_z_flag_set_writes_register() {
        let mut compiler = DynarecCompiler::new();
        // MOVEQ R0, #42    →  03A0_002A
        let block = [0x03A0_002Au32];
        let func = compiler
            .try_compile_imm_block(&block)
            .expect("MOVEQ is supported");

        // CPSR with Z=1 → condition passes → write happens
        let mut gpr = [0u32; 15];
        let mut cpsr: u32 = 1 << 30;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 42);

        // CPSR with Z=0 → condition fails → gpr unchanged
        let mut gpr = [7u32; 15];
        let mut cpsr: u32 = 0;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 7);
    }

    #[test]
    fn movne_complements_moveq() {
        let mut compiler = DynarecCompiler::new();
        // MOVNE R0, #99     →  13A0_0063
        let block = [0x13A0_0063u32];
        let func = compiler
            .try_compile_imm_block(&block)
            .expect("MOVNE is supported");

        // CPSR with Z=0 (not equal) → write happens
        let mut gpr = [0u32; 15];
        let mut cpsr: u32 = 0;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 99);

        // CPSR with Z=1 → condition fails → no write
        let mut gpr = [3u32; 15];
        let mut cpsr: u32 = 1 << 30;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 3);
    }

    #[test]
    fn compile_mov_imm_with_rotation() {
        let mut compiler = DynarecCompiler::new();
        // MOV R3, #0xFF000000 encodes as E3A0_34FF:
        //   imm8 = 0xFF, rot = 4 → rotate right 8 bits → 0xFF000000
        let block = [0xE3A0_34FFu32];
        let func = compiler.try_compile_imm_block(&block).expect("should compile");

        let mut gpr = [0u32; 15];
        { let mut cpsr = 0u32; func(gpr.as_mut_ptr(), &mut cpsr); }
        assert_eq!(gpr[3], 0xFF00_0000);
    }

    #[test]
    fn two_independent_compilations_coexist() {
        let mut compiler = DynarecCompiler::new();
        let f1 = compiler.compile_mov_r0_r1_stub();
        let f2 = compiler.compile_mov_r0_r1_stub();

        let mut gpr_a = [0u32; 15];
        gpr_a[1] = 11;
        f1(gpr_a.as_mut_ptr());

        let mut gpr_b = [0u32; 15];
        gpr_b[1] = 22;
        f2(gpr_b.as_mut_ptr());

        assert_eq!(gpr_a[0], 11);
        assert_eq!(gpr_b[0], 22);
    }
}
