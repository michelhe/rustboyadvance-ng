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
    /// into native code. Supported shapes (all AL-condition, S=0):
    ///
    /// Immediate form (I=1):
    ///   - MOV Rd, #imm          op=1101
    ///   - MVN Rd, #imm          op=1111
    ///   - ADD Rd, Rn, #imm      op=0100
    ///   - SUB Rd, Rn, #imm      op=0010
    ///
    /// Register form (I=0, no shift, no shift-by-register):
    ///   - MOV Rd, Rm
    ///   - MVN Rd, Rm
    ///   - ADD Rd, Rn, Rm
    ///   - SUB Rd, Rn, Rm
    ///
    /// The returned function takes a pointer to the CPU's gpr array.
    /// Returns `None` if any opcode is not one of the supported shapes —
    /// the caller should then fall back to the interpreter replay path
    /// for this block.
    pub fn try_compile_imm_block(
        &mut self,
        opcodes: &[u32],
    ) -> Option<extern "C" fn(*mut u32)> {
        // Validate every opcode up front so we don't create a half-defined
        // function we then have to tear down.
        for &insn in opcodes {
            if Self::decode_supported_dp(insn).is_none() {
                return None;
            }
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));

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

            for &insn in opcodes {
                let dec = Self::decode_supported_dp(insn).expect("pre-validated");
                emit_data_processing_imm(&mut builder, gpr_ptr, dec);
            }

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
            std::mem::transmute::<*const u8, extern "C" fn(*mut u32)>(code)
        })
    }

    /// Classify an ARM opcode as one of the supported data-processing shapes
    /// (immediate or register with no shift). Returns the decoded fields
    /// needed for codegen, or None for any unsupported encoding.
    fn decode_supported_dp(insn: u32) -> Option<DecodedDp> {
        let cond = (insn >> 28) & 0xf;
        if cond != 0xE {
            return None;
        }
        let class = (insn >> 20) & 0xff;
        // Data-processing bits [27:26] = 0b00, S=0 (class bit 0).
        // Immediate form has bit 5 (of class) = 1 (= insn bit 25 = I).
        // Register form has I = 0.
        if (class & 0b1100_0001) != 0b0000_0000 {
            return None;
        }
        let i_bit = (class >> 5) & 1;
        let op = (class >> 1) & 0xf;
        let op = match op {
            0b1101 => DpOp::Mov,
            0b1111 => DpOp::Mvn,
            0b0100 => DpOp::Add,
            0b0010 => DpOp::Sub,
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

        Some(DecodedDp { op, rd, rn, operand2 })
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
/// Extend when adding support for more shapes.
#[derive(Clone, Copy, Debug)]
enum DpOp {
    Mov,
    Mvn,
    Add,
    Sub,
}

/// The second operand of a data-processing instruction.
#[derive(Clone, Copy, Debug)]
enum Operand2 {
    Imm(u32),
    Reg(i32),
}

#[derive(Clone, Copy, Debug)]
struct DecodedDp {
    op: DpOp,
    rd: i32,
    rn: i32,
    operand2: Operand2,
}

/// Emit Cranelift IR for a single decoded data-processing instruction
/// operating on the `gpr` array at `gpr_ptr`. Effect: gpr[rd] = f(gpr[rn], op2).
fn emit_data_processing_imm(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    dec: DecodedDp,
) {
    // Materialize operand2 (either a constant or a register load).
    let op2 = match dec.operand2 {
        Operand2::Imm(v) => builder.ins().iconst(types::I32, v as i64),
        Operand2::Reg(rm) => builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            gpr_ptr,
            Offset32::new(rm * 4),
        ),
    };

    let result = match dec.op {
        DpOp::Mov => op2,
        DpOp::Mvn => {
            // ~op2. For the imm-form we could fold at compile time; the
            // bnot IR op is trivial either way, so keep it uniform.
            builder.ins().bnot(op2)
        }
        DpOp::Add => {
            let rn = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                gpr_ptr,
                Offset32::new(dec.rn * 4),
            );
            builder.ins().iadd(rn, op2)
        }
        DpOp::Sub => {
            let rn = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                gpr_ptr,
                Offset32::new(dec.rn * 4),
            );
            builder.ins().isub(rn, op2)
        }
    };
    builder
        .ins()
        .store(MemFlags::trusted(), result, gpr_ptr, Offset32::new(dec.rd * 4));
}

#[cfg(test)]
mod tests {
    use super::*;

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
        func(gpr.as_mut_ptr());
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
        func(gpr.as_mut_ptr());
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
        func(gpr.as_mut_ptr());
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

        // Block with a non-AL condition — rejected.
        let block = [0x03A0_0001u32]; // MOVEQ R0, #1
        assert!(compiler.try_compile_imm_block(&block).is_none());
    }

    #[test]
    fn compile_mov_imm_with_rotation() {
        let mut compiler = DynarecCompiler::new();
        // MOV R3, #0xFF000000 encodes as E3A0_34FF:
        //   imm8 = 0xFF, rot = 4 → rotate right 8 bits → 0xFF000000
        let block = [0xE3A0_34FFu32];
        let func = compiler.try_compile_imm_block(&block).expect("should compile");

        let mut gpr = [0u32; 15];
        func(gpr.as_mut_ptr());
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
