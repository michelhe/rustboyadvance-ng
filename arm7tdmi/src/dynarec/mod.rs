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
use cranelift_module::{FuncId, Linkage, Module};

/// Rust side trampoline signatures the dynarec calls into for memory ops.
/// The opaque *mut u8 is a "cpu ctx" pointer; whoever constructs the
/// DynarecCompiler supplies trampolines that interpret that pointer the
/// right way for their CPU type. A typical trampoline would cast the
/// pointer to `*mut Arm7tdmiCore<MyBus>` and forward to `cpu.load_32`.
pub type BusLoad32Fn = unsafe extern "C" fn(*mut u8, u32) -> u32;
pub type BusStore32Fn = unsafe extern "C" fn(*mut u8, u32, u32);
pub type BusLoad8Fn = unsafe extern "C" fn(*mut u8, u32) -> u32;
pub type BusStore8Fn = unsafe extern "C" fn(*mut u8, u32, u32);

/// Optional set of bus trampolines. When None the dynarec will refuse to
/// compile anything that needs memory access (returns None from compile
/// paths). When set, compiled blocks can call into these at runtime.
#[derive(Clone, Copy)]
pub struct BusTrampolines {
    pub load_32: BusLoad32Fn,
    pub store_32: BusStore32Fn,
    pub load_8: BusLoad8Fn,
    pub store_8: BusStore8Fn,
}

/// Handle to a Cranelift JIT module. One per CPU instance; freed on CPU drop.
///
/// Wrapping the Cranelift state here keeps the module lifetime tied to the
/// CPU so generated code pages are reclaimed when the emulator tears down.
pub struct DynarecCompiler {
    module: JITModule,
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    next_id: u64,
    /// Resolved imports for the bus trampolines. Only populated when the
    /// compiler was built with `new_with_bus`. The FuncId values are stable
    /// for the lifetime of this compiler and can be re-referenced in every
    /// compiled function via `module.declare_func_in_func`.
    bus_imports: Option<BusImports>,
}

/// Imported function handles registered with the JITModule for the bus
/// trampolines. `declare_func_in_func` re uses these across blocks.
#[derive(Clone, Copy)]
struct BusImports {
    load_32: FuncId,
    store_32: FuncId,
    load_8: FuncId,
    store_8: FuncId,
}

impl DynarecCompiler {
    pub fn new() -> Self {
        Self::build(None)
    }

    /// Build a dynarec compiler wired to a set of bus trampolines. Needed
    /// before any LDR/STR compilation can succeed.
    pub fn new_with_bus(bus: BusTrampolines) -> Self {
        Self::build(Some(bus))
    }

    fn build(bus: Option<BusTrampolines>) -> Self {
        let isa_builder = cranelift_native::builder()
            .expect("host architecture not supported by Cranelift");
        let flag_builder = settings::builder();
        let flags = settings::Flags::new(flag_builder);
        let isa = isa_builder
            .finish(flags)
            .expect("failed to build Cranelift ISA for host");

        let mut jit_builder =
            JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // If the caller passed trampolines, register them as JIT symbols so
        // declare_function(Linkage::Import, ...) can resolve to the real
        // rust side fn pointers at link time.
        if let Some(b) = bus {
            jit_builder.symbol("rba_bus_load_32",  b.load_32  as *const u8);
            jit_builder.symbol("rba_bus_store_32", b.store_32 as *const u8);
            jit_builder.symbol("rba_bus_load_8",   b.load_8   as *const u8);
            jit_builder.symbol("rba_bus_store_8",  b.store_8  as *const u8);
        }

        let mut module = JITModule::new(jit_builder);
        let ctx = module.make_context();

        let bus_imports = bus.map(|_| {
            let ptr_ty = module.isa().pointer_type();
            // load_32: extern "C" fn(*mut u8, u32) -> u32
            let mut sig_load_32 = module.make_signature();
            sig_load_32.params.push(AbiParam::new(ptr_ty));
            sig_load_32.params.push(AbiParam::new(types::I32));
            sig_load_32.returns.push(AbiParam::new(types::I32));
            let load_32 = module
                .declare_function("rba_bus_load_32", Linkage::Import, &sig_load_32)
                .expect("declare load_32 failed");

            // store_32: extern "C" fn(*mut u8, u32, u32)
            let mut sig_store_32 = module.make_signature();
            sig_store_32.params.push(AbiParam::new(ptr_ty));
            sig_store_32.params.push(AbiParam::new(types::I32));
            sig_store_32.params.push(AbiParam::new(types::I32));
            let store_32 = module
                .declare_function("rba_bus_store_32", Linkage::Import, &sig_store_32)
                .expect("declare store_32 failed");

            // load_8: extern "C" fn(*mut u8, u32) -> u32 (zero extended)
            let mut sig_load_8 = module.make_signature();
            sig_load_8.params.push(AbiParam::new(ptr_ty));
            sig_load_8.params.push(AbiParam::new(types::I32));
            sig_load_8.returns.push(AbiParam::new(types::I32));
            let load_8 = module
                .declare_function("rba_bus_load_8", Linkage::Import, &sig_load_8)
                .expect("declare load_8 failed");

            // store_8: extern "C" fn(*mut u8, u32, u32)
            let mut sig_store_8 = module.make_signature();
            sig_store_8.params.push(AbiParam::new(ptr_ty));
            sig_store_8.params.push(AbiParam::new(types::I32));
            sig_store_8.params.push(AbiParam::new(types::I32));
            let store_8 = module
                .declare_function("rba_bus_store_8", Linkage::Import, &sig_store_8)
                .expect("declare store_8 failed");

            BusImports { load_32, store_32, load_8, store_8 }
        });

        DynarecCompiler {
            module,
            builder_context: FunctionBuilderContext::new(),
            ctx,
            next_id: 0,
            bus_imports,
        }
    }

    /// True if this compiler was built with bus trampolines wired up, and
    /// therefore can compile memory ops.
    pub fn has_bus(&self) -> bool {
        self.bus_imports.is_some()
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

    /// Variant of `try_compile_imm_block` that accepts an optional trailing
    /// ARM B / BL as a block terminator. The returned function writes the
    /// new PC value (already adjusted to the ARM branch's PC+8+offset<<2
    /// semantics) into `*pc_out` and returns 1 iff the branch was taken,
    /// otherwise returns 0 and leaves `*pc_out` untouched.
    ///
    /// `entry_pc` is the address of the first instruction in `opcodes` as
    /// the ARM pipeline sees it (i.e. the pc the interpreter would be at
    /// while decoding the first instr, which equals the instruction's own
    /// address plus 8 per ARM pipeline convention). We fold that into the
    /// generated code so the runtime doesn't need to know where the block
    /// lived.
    ///
    /// Returns None if any instruction other than the optional trailing B/BL
    /// isn't a supported DP shape, or if a B/BL shows up anywhere other
    /// than the last slot.
    pub fn try_compile_block_with_branch(
        &mut self,
        opcodes: &[u32],
        entry_pc: u32,
    ) -> Option<extern "C" fn(*mut u32, *mut u32, *mut u32) -> u32> {
        if opcodes.is_empty() {
            return None;
        }

        // Branch may only appear as the last instruction.
        let (body_opcodes, tail) = opcodes.split_at(opcodes.len() - 1);
        let tail_insn = tail[0];

        // First: every body instruction must be a DP shape we can emit.
        for &insn in body_opcodes {
            if Self::decode_supported_dp(insn).is_none() {
                return None;
            }
            if Self::decode_branch(insn).is_some() {
                // B/BL found mid block; reject until we know how to handle
                // mid block early return vs scheduler interleaving cleanly.
                return None;
            }
        }

        // The tail can be either a DP shape (no branch) OR a B/BL.
        let tail_is_dp = Self::decode_supported_dp(tail_insn).is_some();
        let tail_is_branch = Self::decode_branch(tail_insn);
        if !tail_is_dp && tail_is_branch.is_none() {
            return None;
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // gpr_ptr
        sig.params.push(AbiParam::new(ptr_type)); // cpsr_ptr
        sig.params.push(AbiParam::new(ptr_type)); // pc_out
        sig.returns.push(AbiParam::new(types::I32));

        self.next_id += 1;
        let name = format!("dynarec_branch_block_{}", self.next_id);
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
            let pc_out = builder.block_params(entry)[2];

            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder.ins().load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            // Pre-compute the entry pc of each instruction as if the ARM
            // pipeline were live. The first body instr is at entry_pc-8 from
            // the branch's perspective (since ARM PC during execute is
            // current_instr+8). For the tail branch we use entry_pc + 8 +
            // (body_len * 4), matching how the interpreter would have
            // advanced pc by the time B itself executes.
            let body_len = body_opcodes.len() as u32;

            for (i, &insn) in body_opcodes.iter().enumerate() {
                let dec = Self::decode_supported_dp(insn).expect("pre-validated");
                // Reject MOV/etc to R15 in compiled path: that would flush
                // the pipeline and we don't model it yet.
                if dec.rd == 15 || (matches!(dec.operand2, Operand2::Reg(15))) {
                    return None;
                }
                let _ = i;
                emit_conditional_instr(&mut builder, gpr_ptr, cpsr_var, dec);
            }

            // Fall through vs branch-taken return path.
            let took_branch_var = builder.declare_var(types::I32);
            let zero = builder.ins().iconst(types::I32, 0);
            builder.def_var(took_branch_var, zero);

            if let Some(br) = tail_is_branch {
                // pc of the B instruction itself, in linear layout from block
                // entry.
                let branch_insn_pc = entry_pc.wrapping_add(body_len.wrapping_mul(4));
                // ARM B semantics: new_pc = (pc_of_B + 8) + sign_extend(imm24) * 4.
                let target = branch_insn_pc
                    .wrapping_add(8)
                    .wrapping_add((br.offset24_signed as i32 * 4) as u32);

                let cond_pass = emit_cond_check(&mut builder, cpsr_var, br.cond);
                let taken = builder.create_block();
                let not_taken = builder.create_block();
                builder.ins().brif(cond_pass, taken, &[], not_taken, &[]);

                builder.switch_to_block(taken);
                builder.seal_block(taken);
                // If BL, write LR = pc_of_B + 4.
                if br.link {
                    let lr_val = builder
                        .ins()
                        .iconst(types::I32, (branch_insn_pc.wrapping_add(4)) as i64);
                    builder.ins().store(
                        MemFlags::trusted(),
                        lr_val,
                        gpr_ptr,
                        Offset32::new(14 * 4),
                    );
                }
                let target_val = builder.ins().iconst(types::I32, target as i64);
                builder.ins().store(MemFlags::trusted(), target_val, pc_out, 0);
                let one = builder.ins().iconst(types::I32, 1);
                builder.def_var(took_branch_var, one);
                builder.ins().jump(not_taken, &[]);

                builder.switch_to_block(not_taken);
                builder.seal_block(not_taken);
            } else {
                // Trailing DP instruction, same as body.
                let dec = Self::decode_supported_dp(tail_insn).expect("pre-validated");
                if dec.rd == 15 || matches!(dec.operand2, Operand2::Reg(15)) {
                    return None;
                }
                emit_conditional_instr(&mut builder, gpr_ptr, cpsr_var, dec);
            }

            let cpsr_final = builder.use_var(cpsr_var);
            builder
                .ins()
                .store(MemFlags::trusted(), cpsr_final, cpsr_ptr, 0);

            let ret_val = builder.use_var(took_branch_var);
            builder.ins().return_(&[ret_val]);
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
        Some(unsafe {
            std::mem::transmute::<
                *const u8,
                extern "C" fn(*mut u32, *mut u32, *mut u32) -> u32,
            >(code)
        })
    }

    /// Classify an ARM opcode as B or BL. Returns None for everything else.
    /// Encoding (ARMv4):
    ///   cond[31:28] | 101L[27:24] | imm24[23:0]
    /// L=0 -> B, L=1 -> BL. imm24 is a signed 24-bit value; the effective
    /// offset is sign_extend(imm24) << 2, added to PC+8.
    fn decode_branch(insn: u32) -> Option<DecodedBranch> {
        let cond_bits = (insn >> 28) & 0xf;
        if cond_bits == 0xF {
            return None;
        }
        // Top 7 bits of the non-cond field must be 0b1010xxx (B) or 0b1011xxx (BL).
        if (insn >> 25) & 0b111 != 0b101 {
            return None;
        }
        let link = ((insn >> 24) & 1) != 0;
        // Sign-extend the 24 bit immediate into i32.
        let raw = insn & 0x00ff_ffff;
        let signed = ((raw as i32) << 8) >> 8; // arithmetic shift to sign-extend
        Some(DecodedBranch {
            cond: ArmCond::from_bits(cond_bits as u8),
            link,
            offset24_signed: signed,
        })
    }

    /// Compile a block that mixes supported data processing shapes with ARM
    /// LDR / STR immediate (word size, pre indexed, no writeback). Calls
    /// into the bus trampolines the compiler was built with.
    ///
    /// Signature:
    ///   extern "C" fn(
    ///     gpr_ptr: *mut u32,
    ///     cpsr_ptr: *mut u32,
    ///     cpu_ctx: *mut u8,   // opaque context passed through to trampolines
    ///   )
    ///
    /// Returns None if the compiler was built without bus trampolines, or if
    /// any opcode isn't a supported DP or mem shape.
    pub fn try_compile_mem_block(
        &mut self,
        opcodes: &[u32],
    ) -> Option<extern "C" fn(*mut u32, *mut u32, *mut u8)> {
        let imports = self.bus_imports?;

        // Classify each instr as DP or Mem. Reject early if anything is
        // unsupported so we don't half emit.
        enum BlockItem {
            Dp(DecodedDp),
            Mem(DecodedMem),
        }
        let mut items = Vec::with_capacity(opcodes.len());
        for &insn in opcodes {
            if let Some(m) = Self::decode_mem_immediate(insn) {
                // PC relative loads aren't emitted yet; reject.
                if m.rn == 15 || m.rd == 15 {
                    return None;
                }
                items.push(BlockItem::Mem(m));
            } else if let Some(d) = Self::decode_supported_dp(insn) {
                if d.rd == 15 || matches!(d.operand2, Operand2::Reg(15)) {
                    return None;
                }
                items.push(BlockItem::Dp(d));
            } else {
                return None;
            }
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // gpr_ptr
        sig.params.push(AbiParam::new(ptr_type)); // cpsr_ptr
        sig.params.push(AbiParam::new(ptr_type)); // cpu_ctx

        self.next_id += 1;
        let name = format!("dynarec_mem_block_{}", self.next_id);
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
            let cpu_ctx = builder.block_params(entry)[2];

            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder.ins().load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            // Reference imports once in this function.
            let load_32_ref =
                self.module.declare_func_in_func(imports.load_32, builder.func);
            let store_32_ref =
                self.module.declare_func_in_func(imports.store_32, builder.func);
            let load_8_ref =
                self.module.declare_func_in_func(imports.load_8, builder.func);
            let store_8_ref =
                self.module.declare_func_in_func(imports.store_8, builder.func);

            for item in &items {
                match item {
                    BlockItem::Dp(dec) => {
                        emit_conditional_instr(&mut builder, gpr_ptr, cpsr_var, *dec);
                    }
                    BlockItem::Mem(mem) => {
                        emit_conditional_mem(
                            &mut builder,
                            gpr_ptr,
                            cpsr_var,
                            cpu_ctx,
                            load_32_ref,
                            store_32_ref,
                            load_8_ref,
                            store_8_ref,
                            *mem,
                        );
                    }
                }
            }

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
        Some(unsafe {
            std::mem::transmute::<*const u8, extern "C" fn(*mut u32, *mut u32, *mut u8)>(
                code,
            )
        })
    }

    /// Classify an ARM LDR / STR immediate opcode. Encoding:
    ///   cond[31:28] | 01[27:26] | 0[25=I] P U B W L [Rn] [Rd] imm12
    /// Required: pre indexed (P=1), no writeback (W=0), immediate form (I=0).
    /// B=0 is word, B=1 is unsigned byte. U picks add vs subtract.
    fn decode_mem_immediate(insn: u32) -> Option<DecodedMem> {
        let cond_bits = (insn >> 28) & 0xf;
        if cond_bits == 0xF {
            return None;
        }
        // Bits [27:26] must be 01 for load/store single.
        if (insn >> 26) & 0b11 != 0b01 {
            return None;
        }
        // I must be 0 (immediate form, not register). Bit 25.
        if (insn >> 25) & 1 != 0 {
            return None;
        }
        let p = (insn >> 24) & 1 != 0;
        let u = (insn >> 23) & 1 != 0;
        let b = (insn >> 22) & 1 != 0;
        let w = (insn >> 21) & 1 != 0;
        let l = (insn >> 20) & 1 != 0;

        // Pre indexed, no writeback. U and B both accepted now.
        if !p || w {
            return None;
        }

        let rn = ((insn >> 16) & 0xf) as i32;
        let rd = ((insn >> 12) & 0xf) as i32;
        let imm12 = insn & 0xfff;

        Some(DecodedMem {
            cond: ArmCond::from_bits(cond_bits as u8),
            load: l,
            byte: b,
            add: u,
            rd,
            rn,
            offset: imm12,
        })
    }

    /// Compile a block of Thumb 16 bit opcodes. Supports:
    ///   - format 1: LSL/LSR/ASR Rd, Rs, #imm5 (with shifter carry)
    ///   - format 2: ADD/SUB Rd, Rs, Rn/imm3
    ///   - format 3: MOV/CMP/ADD/SUB Rd, #imm8
    ///   - format 4 logical subset: AND/EOR/ORR/BIC/MVN/TST/CMP/CMN Rd, Rs
    ///   - format 5 non-branch: ADD/CMP/MOV Hi registers (no flag updates
    ///     on ADD/MOV, CMP still sets flags). BX deferred to the branch
    ///     block path.
    ///
    ///   001_oo_ddd_iiiiiiii
    ///     oo = 00 MOV, 01 CMP, 10 ADD, 11 SUB
    ///     Rd = ddd
    ///     imm8 = low byte
    ///
    /// All four mnemonics update CPSR.NZCV unconditionally (Thumb has no
    /// per instruction cond field outside IT blocks, which ARMv4 doesn't
    /// have anyway). CMP does not writeback. MOV just sets N and Z; ADD
    /// and SUB update full NZCV. The compiled fn signature matches the ARM
    /// DP one so the caller logic is the same:
    ///
    ///   extern "C" fn(*mut u32 gpr, *mut u32 cpsr)
    ///
    /// Returns None for any unsupported encoding.
    pub fn try_compile_thumb_block(
        &mut self,
        opcodes: &[u16],
    ) -> Option<extern "C" fn(*mut u32, *mut u32)> {
        // Each opcode must classify as one of the supported formats.
        enum ThumbItem {
            F1(DecodedThumb1),
            F2(DecodedThumb2),
            F3(DecodedThumb3),
            F4(DecodedThumb4),
            F5(DecodedThumb5),
        }
        let mut items: Vec<ThumbItem> = Vec::with_capacity(opcodes.len());
        for &op in opcodes {
            // Format 1 has to be tried before format 2 because 00011xx... is
            // format 2 but 000xx... otherwise is format 1. decode_thumb_format1
            // explicitly rejects the format 2 bit pattern.
            if let Some(d) = Self::decode_thumb_format1(op) {
                items.push(ThumbItem::F1(d));
            } else if let Some(d) = Self::decode_thumb_format2(op) {
                items.push(ThumbItem::F2(d));
            } else if let Some(d) = Self::decode_thumb_format3(op) {
                items.push(ThumbItem::F3(d));
            } else if let Some(d) = Self::decode_thumb_format4_logical(op) {
                items.push(ThumbItem::F4(d));
            } else if let Some(d) = Self::decode_thumb_format5_non_branch(op) {
                items.push(ThumbItem::F5(d));
            } else {
                return None;
            }
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));

        self.next_id += 1;
        let name = format!("dynarec_thumb_block_{}", self.next_id);
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

            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder.ins().load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            for item in &items {
                match item {
                    ThumbItem::F1(d) => emit_thumb_format1(&mut builder, gpr_ptr, cpsr_var, *d),
                    ThumbItem::F2(d) => emit_thumb_format2(&mut builder, gpr_ptr, cpsr_var, *d),
                    ThumbItem::F3(d) => emit_thumb_format3(&mut builder, gpr_ptr, cpsr_var, *d),
                    ThumbItem::F4(d) => emit_thumb_format4_logical(&mut builder, gpr_ptr, cpsr_var, *d),
                    ThumbItem::F5(d) => emit_thumb_format5_non_branch(&mut builder, gpr_ptr, cpsr_var, *d),
                }
            }

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
        Some(unsafe {
            std::mem::transmute::<*const u8, extern "C" fn(*mut u32, *mut u32)>(code)
        })
    }

    /// Thumb variant that mixes DP shapes with memory ops. Calls into the
    /// bus trampolines registered at compiler construction time, like the
    /// ARM `try_compile_mem_block`. Returns None if the compiler was built
    /// without `new_with_bus`, or if any opcode is not a supported shape.
    pub fn try_compile_thumb_mem_block(
        &mut self,
        opcodes: &[u16],
    ) -> Option<extern "C" fn(*mut u32, *mut u32, *mut u8)> {
        let imports = self.bus_imports?;

        enum MemItem {
            F1(DecodedThumb1),
            F2(DecodedThumb2),
            F3(DecodedThumb3),
            F4(DecodedThumb4),
            F5(DecodedThumb5),
            F9(DecodedThumb9),
            F11(DecodedThumb11),
            F14(DecodedThumb14),
        }
        let mut items = Vec::with_capacity(opcodes.len());
        for &op in opcodes {
            if let Some(d) = Self::decode_thumb_format14_non_pc(op) {
                items.push(MemItem::F14(d));
            } else if let Some(d) = Self::decode_thumb_format11(op) {
                items.push(MemItem::F11(d));
            } else if let Some(d) = Self::decode_thumb_format9(op) {
                items.push(MemItem::F9(d));
            } else if let Some(d) = Self::decode_thumb_format1(op) {
                items.push(MemItem::F1(d));
            } else if let Some(d) = Self::decode_thumb_format2(op) {
                items.push(MemItem::F2(d));
            } else if let Some(d) = Self::decode_thumb_format3(op) {
                items.push(MemItem::F3(d));
            } else if let Some(d) = Self::decode_thumb_format4_logical(op) {
                items.push(MemItem::F4(d));
            } else if let Some(d) = Self::decode_thumb_format5_non_branch(op) {
                items.push(MemItem::F5(d));
            } else {
                return None;
            }
        }

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));

        self.next_id += 1;
        let name = format!("dynarec_thumb_mem_block_{}", self.next_id);
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
            let cpu_ctx = builder.block_params(entry)[2];

            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder.ins().load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            let load_32_ref =
                self.module.declare_func_in_func(imports.load_32, builder.func);
            let store_32_ref =
                self.module.declare_func_in_func(imports.store_32, builder.func);
            let load_8_ref =
                self.module.declare_func_in_func(imports.load_8, builder.func);
            let store_8_ref =
                self.module.declare_func_in_func(imports.store_8, builder.func);

            for item in &items {
                match item {
                    MemItem::F1(d) => emit_thumb_format1(&mut builder, gpr_ptr, cpsr_var, *d),
                    MemItem::F2(d) => emit_thumb_format2(&mut builder, gpr_ptr, cpsr_var, *d),
                    MemItem::F3(d) => emit_thumb_format3(&mut builder, gpr_ptr, cpsr_var, *d),
                    MemItem::F4(d) => emit_thumb_format4_logical(&mut builder, gpr_ptr, cpsr_var, *d),
                    MemItem::F5(d) => emit_thumb_format5_non_branch(&mut builder, gpr_ptr, cpsr_var, *d),
                    MemItem::F9(d) => emit_thumb_format9(
                        &mut builder, gpr_ptr, cpu_ctx,
                        load_32_ref, store_32_ref, load_8_ref, store_8_ref,
                        *d,
                    ),
                    MemItem::F11(d) => emit_thumb_format11(
                        &mut builder, gpr_ptr, cpu_ctx,
                        load_32_ref, store_32_ref,
                        *d,
                    ),
                    MemItem::F14(d) => emit_thumb_format14(
                        &mut builder, gpr_ptr, cpu_ctx,
                        load_32_ref, store_32_ref,
                        *d,
                    ),
                }
            }

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
        Some(unsafe {
            std::mem::transmute::<
                *const u8,
                extern "C" fn(*mut u32, *mut u32, *mut u8),
            >(code)
        })
    }

    /// Classify a Thumb format 14 PUSH / POP (non PC variant):
    ///   1011_L_10_R_rrrrrrrr
    ///     L = 0 PUSH, 1 POP
    ///     R = include LR (PUSH) or PC (POP).
    ///     rrrrrrrr = R0..R7 inclusion bitmap.
    /// POP with R=1 (POP{PC}) is a block terminator and gets handled in the
    /// branch block path, so it's rejected here.
    /// Empty register list (including R) is UNPREDICTABLE per spec,
    /// rejected.
    fn decode_thumb_format14_non_pc(op: u16) -> Option<DecodedThumb14> {
        if (op >> 12) & 0xF != 0b1011 {
            return None;
        }
        let load = (op >> 11) & 1 != 0;
        // Bits [10:9] must be 10.
        if (op >> 9) & 0b11 != 0b10 {
            return None;
        }
        let extra = (op >> 8) & 1 != 0;
        let reg_list = (op & 0xff) as u8;

        // Defer POP{PC} to try_compile_thumb_block_with_branch.
        if load && extra {
            return None;
        }
        // Empty effective register list is UNPREDICTABLE.
        if reg_list == 0 && !extra {
            return None;
        }

        Some(DecodedThumb14 {
            push: !load,
            extra_reg: extra,
            reg_list,
        })
    }

    /// Classify a Thumb format 11 SP relative LDR/STR:
    ///   1001_L_ddd_iiiiiiii
    ///     L = 0 STR, 1 LDR. Always word sized. imm8 is scaled by 4.
    /// addr = gpr[13] (SP) + imm8 * 4.
    fn decode_thumb_format11(op: u16) -> Option<DecodedThumb11> {
        if (op >> 12) & 0xF != 0b1001 {
            return None;
        }
        let load = (op >> 11) & 1 != 0;
        let rd = ((op >> 8) & 0b111) as i32;
        let imm8 = (op & 0xff) as u32;
        let offset = imm8 * 4;
        Some(DecodedThumb11 { load, rd, offset })
    }

    /// Classify a Thumb format 9 LDR/STR immediate offset:
    ///   011_B_L_iiiii_sss_ddd
    /// B = 0 word (offset = imm5 * 4), B = 1 byte (offset = imm5).
    /// L = 0 STR, 1 LDR.
    fn decode_thumb_format9(op: u16) -> Option<DecodedThumb9> {
        if (op >> 13) & 0b111 != 0b011 {
            return None;
        }
        let byte = (op >> 12) & 1 != 0;
        let load = (op >> 11) & 1 != 0;
        let imm5 = ((op >> 6) & 0b11111) as u32;
        let rs = ((op >> 3) & 0b111) as i32;
        let rd = (op & 0b111) as i32;
        let offset = if byte { imm5 } else { imm5 * 4 };
        Some(DecodedThumb9 { load, byte, offset, rs, rd })
    }

    /// Thumb variant of `try_compile_block_with_branch`: compiles a block
    /// whose body is supported Thumb shapes plus an optional trailing BX Rs.
    /// BX is a block terminator that writes the target pc (preserving bit 0
    /// as the ARM/Thumb mode signal, per ARM BX convention) into *pc_out
    /// and returns 1. If there's no BX tail the block compiles as usual
    /// and returns 0.
    pub fn try_compile_thumb_block_with_branch(
        &mut self,
        opcodes: &[u16],
        entry_pc: u32,
    ) -> Option<extern "C" fn(*mut u32, *mut u32, *mut u32) -> u32> {
        if opcodes.is_empty() {
            return None;
        }

        let (body_opcodes, tail) = opcodes.split_at(opcodes.len() - 1);
        let tail_insn = tail[0];

        // Classify body: every non-tail opcode must be one of the supported
        // straight-line Thumb shapes. Branching shapes (BX, fmt 16, fmt 18)
        // only allowed in the tail slot.
        enum BodyItem {
            F1(DecodedThumb1),
            F2(DecodedThumb2),
            F3(DecodedThumb3),
            F4(DecodedThumb4),
            F5(DecodedThumb5),
        }
        let mut body: Vec<BodyItem> = Vec::with_capacity(body_opcodes.len());
        for &op in body_opcodes {
            if Self::decode_thumb_bx(op).is_some()
                || Self::decode_thumb_format16(op).is_some()
                || Self::decode_thumb_format18(op).is_some()
            {
                return None;
            }
            if let Some(d) = Self::decode_thumb_format1(op) {
                body.push(BodyItem::F1(d));
            } else if let Some(d) = Self::decode_thumb_format2(op) {
                body.push(BodyItem::F2(d));
            } else if let Some(d) = Self::decode_thumb_format3(op) {
                body.push(BodyItem::F3(d));
            } else if let Some(d) = Self::decode_thumb_format4_logical(op) {
                body.push(BodyItem::F4(d));
            } else if let Some(d) = Self::decode_thumb_format5_non_branch(op) {
                body.push(BodyItem::F5(d));
            } else {
                return None;
            }
        }

        // Tail classification: prefer branch shapes over body shapes since
        // the body also accepts fmt 5 ADD/CMP/MOV which share the 010001
        // prefix. BX / fmt 16 / fmt 18 each have their own distinguishing
        // prefix so there's no overlap to worry about.
        let tail_bx = Self::decode_thumb_bx(tail_insn);
        let tail_pc_branch = Self::decode_thumb_format18(tail_insn)
            .or_else(|| Self::decode_thumb_format16(tail_insn));
        let tail_body = if tail_bx.is_none() && tail_pc_branch.is_none() {
            if let Some(d) = Self::decode_thumb_format1(tail_insn) {
                Some(BodyItem::F1(d))
            } else if let Some(d) = Self::decode_thumb_format2(tail_insn) {
                Some(BodyItem::F2(d))
            } else if let Some(d) = Self::decode_thumb_format3(tail_insn) {
                Some(BodyItem::F3(d))
            } else if let Some(d) = Self::decode_thumb_format4_logical(tail_insn) {
                Some(BodyItem::F4(d))
            } else if let Some(d) = Self::decode_thumb_format5_non_branch(tail_insn) {
                Some(BodyItem::F5(d))
            } else {
                return None;
            }
        } else {
            None
        };

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns.push(AbiParam::new(types::I32));

        self.next_id += 1;
        let name = format!("dynarec_thumb_branch_block_{}", self.next_id);
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
            let pc_out = builder.block_params(entry)[2];

            let cpsr_var = builder.declare_var(types::I32);
            let cpsr_initial =
                builder.ins().load(types::I32, MemFlags::trusted(), cpsr_ptr, 0);
            builder.def_var(cpsr_var, cpsr_initial);

            for item in &body {
                match item {
                    BodyItem::F1(d) => emit_thumb_format1(&mut builder, gpr_ptr, cpsr_var, *d),
                    BodyItem::F2(d) => emit_thumb_format2(&mut builder, gpr_ptr, cpsr_var, *d),
                    BodyItem::F3(d) => emit_thumb_format3(&mut builder, gpr_ptr, cpsr_var, *d),
                    BodyItem::F4(d) => emit_thumb_format4_logical(&mut builder, gpr_ptr, cpsr_var, *d),
                    BodyItem::F5(d) => emit_thumb_format5_non_branch(&mut builder, gpr_ptr, cpsr_var, *d),
                }
            }

            let took_branch_var = builder.declare_var(types::I32);
            let zero = builder.ins().iconst(types::I32, 0);
            builder.def_var(took_branch_var, zero);

            if let Some(bx) = tail_bx {
                // target = gpr[bx.rs]. Preserve bit 0 (mode signal).
                let target = builder.ins().load(
                    types::I32,
                    MemFlags::trusted(),
                    gpr_ptr,
                    Offset32::new(bx.rs * 4),
                );
                builder.ins().store(MemFlags::trusted(), target, pc_out, 0);
                let one = builder.ins().iconst(types::I32, 1);
                builder.def_var(took_branch_var, one);
            } else if let Some(br) = tail_pc_branch {
                // Static target at codegen time.
                // pc_of_branch_in_block = entry_pc + 2 * (body_len)
                // current_pc_at_execute = pc_of_branch + 4 (Thumb pipeline)
                // final_target           = current_pc + (offset << 1)
                let body_len = body.len() as u32;
                let branch_pc = entry_pc.wrapping_add(body_len.wrapping_mul(2));
                let target = branch_pc
                    .wrapping_add(4)
                    .wrapping_add((br.offset_signed << 1) as u32)
                    | 1; // stay in Thumb mode
                let cond_pass = emit_cond_check(&mut builder, cpsr_var, br.cond);
                let taken = builder.create_block();
                let not_taken = builder.create_block();
                builder.ins().brif(cond_pass, taken, &[], not_taken, &[]);

                builder.switch_to_block(taken);
                builder.seal_block(taken);
                let t_val = builder.ins().iconst(types::I32, target as i64);
                builder.ins().store(MemFlags::trusted(), t_val, pc_out, 0);
                let one = builder.ins().iconst(types::I32, 1);
                builder.def_var(took_branch_var, one);
                builder.ins().jump(not_taken, &[]);

                builder.switch_to_block(not_taken);
                builder.seal_block(not_taken);
            } else if let Some(item) = tail_body {
                match item {
                    BodyItem::F1(d) => emit_thumb_format1(&mut builder, gpr_ptr, cpsr_var, d),
                    BodyItem::F2(d) => emit_thumb_format2(&mut builder, gpr_ptr, cpsr_var, d),
                    BodyItem::F3(d) => emit_thumb_format3(&mut builder, gpr_ptr, cpsr_var, d),
                    BodyItem::F4(d) => emit_thumb_format4_logical(&mut builder, gpr_ptr, cpsr_var, d),
                    BodyItem::F5(d) => emit_thumb_format5_non_branch(&mut builder, gpr_ptr, cpsr_var, d),
                }
            }

            let cpsr_final = builder.use_var(cpsr_var);
            builder
                .ins()
                .store(MemFlags::trusted(), cpsr_final, cpsr_ptr, 0);
            let ret_val = builder.use_var(took_branch_var);
            builder.ins().return_(&[ret_val]);
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
        Some(unsafe {
            std::mem::transmute::<
                *const u8,
                extern "C" fn(*mut u32, *mut u32, *mut u32) -> u32,
            >(code)
        })
    }

    /// Classify a Thumb format 18 unconditional branch:
    ///   11100_iiiiiiiiiii   (11 bit signed offset)
    /// Target = current pc (= instr + 4) + sign_extend(imm11) << 1.
    fn decode_thumb_format18(op: u16) -> Option<DecodedThumbPcBranch> {
        if (op >> 11) & 0b11111 != 0b11100 {
            return None;
        }
        let raw = (op & 0x07FF) as i32;
        let signed = (raw << 21) >> 21; // sign extend 11 bits
        Some(DecodedThumbPcBranch {
            cond: ArmCond::Al,
            offset_signed: signed,
        })
    }

    /// Classify a Thumb format 16 conditional branch:
    ///   1101_cccc_iiiiiiii
    /// cond 0xE (AL) is reserved here (that'd be format 18), cond 0xF is
    /// SWI (format 17). Both rejected. Target = pc + sign_extend(imm8) << 1.
    fn decode_thumb_format16(op: u16) -> Option<DecodedThumbPcBranch> {
        if (op >> 12) & 0xF != 0b1101 {
            return None;
        }
        let cond_bits = ((op >> 8) & 0xF) as u8;
        if cond_bits == 0xE || cond_bits == 0xF {
            return None;
        }
        let raw = (op & 0xFF) as i32;
        let signed = (raw << 24) >> 24; // sign extend 8 bits
        Some(DecodedThumbPcBranch {
            cond: ArmCond::from_bits(cond_bits),
            offset_signed: signed,
        })
    }

    /// Classify a Thumb BX (format 5 with oo=11). Encoding:
    ///   010001_11_0_H2_sss_000
    /// H1 and the low 3 bits are SBZ (should be zero); if set, we reject
    /// rather than silently compiling UNPREDICTABLE behavior.
    fn decode_thumb_bx(op: u16) -> Option<DecodedThumbBx> {
        if (op >> 10) & 0b111111 != 0b010001 {
            return None;
        }
        let oo = (op >> 8) & 0b11;
        if oo != 0b11 {
            return None;
        }
        let h1 = (op >> 7) & 1;
        if h1 != 0 {
            return None; // SBZ
        }
        if op & 0b111 != 0 {
            return None; // SBZ low bits
        }
        let h2 = (op >> 6) & 1;
        let rs_raw = (op >> 3) & 0b111;
        let rs = (rs_raw | (h2 << 3)) as i32;
        // PC as source would mean "BX PC" which flushes into a known
        // constant pc + 4 (Thumb) / pc + 8 (ARM). Deferred for now.
        if rs == 15 {
            return None;
        }
        Some(DecodedThumbBx { rs })
    }

    /// Classify a Thumb 16 bit opcode as format 5 (Hi register op),
    /// non-branch mnemonics only. Encoding:
    ///   010001_oo_H1_H2_sss_ddd
    ///     oo = 00 ADD, 01 CMP, 10 MOV, 11 BX
    ///     H1 selects Rd in upper bank (R8-R15), H2 same for Rs.
    ///     Full reg index = (H1<<3) | ddd  (and similarly H2|sss).
    ///
    /// Rejected here (return None):
    ///   - oo=11 (BX). Deferred to try_compile_block_with_branch.
    ///   - Any reg index = 15 (PC). Handling PC as source would need pc
    ///     folding; as dest it would flush the pipeline. Either way not
    ///     in the straight line compile path.
    ///   - oo=00/01/10 with both H1=0 and H2=0. That encoding is
    ///     UNPREDICTABLE per the ARM spec; bail to the interpreter.
    fn decode_thumb_format5_non_branch(op: u16) -> Option<DecodedThumb5> {
        if (op >> 10) & 0b111111 != 0b010001 {
            return None;
        }
        let oo = (op >> 8) & 0b11;
        if oo == 0b11 {
            return None; // BX handled elsewhere
        }
        let h1 = (op >> 7) & 1;
        let h2 = (op >> 6) & 1;
        if h1 == 0 && h2 == 0 {
            return None; // UNPREDICTABLE per spec
        }
        let rd_raw = op & 0b111;
        let rs_raw = (op >> 3) & 0b111;
        let rd = (rd_raw | (h1 << 3)) as i32;
        let rs = (rs_raw | (h2 << 3)) as i32;
        if rd == 15 || rs == 15 {
            return None;
        }
        let mnemonic = match oo {
            0b00 => Thumb5Op::Add,
            0b01 => Thumb5Op::Cmp,
            0b10 => Thumb5Op::Mov,
            _ => unreachable!(),
        };
        Some(DecodedThumb5 { op: mnemonic, rd, rs })
    }

    /// Classify a Thumb 16 bit opcode as format 1 (LSL/LSR/ASR Rd, Rs,
    /// #imm5). Encoding:
    ///   000_oo_iiiii_sss_ddd
    ///     oo = 00 LSL, 01 LSR, 10 ASR  (11 is format 2 add/sub, rejected)
    ///     imm5 = iiiii
    ///     Rs = sss, Rd = ddd
    fn decode_thumb_format1(op: u16) -> Option<DecodedThumb1> {
        if (op >> 13) & 0b111 != 0b000 {
            return None;
        }
        let oo = (op >> 11) & 0b11;
        if oo == 0b11 {
            // That's format 2.
            return None;
        }
        let imm5 = ((op >> 6) & 0b11111) as u32;
        let rs = ((op >> 3) & 0b111) as i32;
        let rd = (op & 0b111) as i32;
        let kind = match oo {
            0b00 => ShiftKind::Lsl,
            0b01 => ShiftKind::Lsr,
            0b10 => ShiftKind::Asr,
            _ => unreachable!(),
        };
        Some(DecodedThumb1 { kind, imm5, rs, rd })
    }

    /// Classify a Thumb 16 bit opcode as one of the format 4 logical
    /// subset: AND/EOR/ORR/BIC/MVN/TST/CMP/CMN Rd, Rs. Encoding:
    ///   010000_oooo_sss_ddd
    /// Only the logical / compare mnemonics are handled here. Shift ops
    /// (LSL/LSR/ASR/ROR) need barrel shifter carry handling, ADC/SBC need
    /// carry in, NEG needs signed negation semantics, MUL is its own
    /// timing. Those are all rejected for now.
    fn decode_thumb_format4_logical(op: u16) -> Option<DecodedThumb4> {
        if (op >> 10) & 0b111111 != 0b010000 {
            return None;
        }
        let op_bits = (op >> 6) & 0xf;
        let rs = ((op >> 3) & 0b111) as i32;
        let rd = (op & 0b111) as i32;
        let mnemonic = match op_bits {
            0b0000 => Thumb4Op::And,
            0b0001 => Thumb4Op::Eor,
            0b1000 => Thumb4Op::Tst,
            0b1010 => Thumb4Op::Cmp,
            0b1011 => Thumb4Op::Cmn,
            0b1100 => Thumb4Op::Orr,
            0b1110 => Thumb4Op::Bic,
            0b1111 => Thumb4Op::Mvn,
            // Unsupported: 0010 LSL, 0011 LSR, 0100 ASR, 0101 ADC,
            // 0110 SBC, 0111 ROR, 1001 NEG, 1101 MUL.
            _ => return None,
        };
        Some(DecodedThumb4 { op: mnemonic, rd, rs })
    }

    /// Classify a Thumb 16 bit opcode as format 2 (ADD/SUB Rd, Rs, Rn
    /// OR ADD/SUB Rd, Rs, #imm3). Encoding:
    ///   00011_I_Op_nnn_sss_ddd
    ///     I = 0 register (Rn = nnn), 1 immediate (imm3 = nnn)
    ///     Op = 0 ADD, 1 SUB
    ///     Rs = sss, Rd = ddd
    fn decode_thumb_format2(op: u16) -> Option<DecodedThumb2> {
        if (op >> 11) & 0b11111 != 0b00011 {
            return None;
        }
        let imm_form = (op >> 10) & 1 != 0;
        let sub = (op >> 9) & 1 != 0;
        let rn_or_imm = ((op >> 6) & 0b111) as u32;
        let rs = ((op >> 3) & 0b111) as i32;
        let rd = (op & 0b111) as i32;
        let operand = if imm_form {
            Thumb2Operand::Imm3(rn_or_imm)
        } else {
            Thumb2Operand::Reg(rn_or_imm as i32)
        };
        Some(DecodedThumb2 {
            sub,
            rd,
            rs,
            operand,
        })
    }

    /// Classify a Thumb 16 bit opcode as format 3 (MOV/CMP/ADD/SUB Rd,
    /// #imm8). Top 3 bits of the opcode must be 0b001.
    fn decode_thumb_format3(op: u16) -> Option<DecodedThumb3> {
        if (op >> 13) & 0b111 != 0b001 {
            return None;
        }
        let oo = (op >> 11) & 0b11;
        let rd = ((op >> 8) & 0b111) as i32;
        let imm8 = (op & 0xff) as u32;
        let mnemonic = match oo {
            0b00 => Thumb3Op::Mov,
            0b01 => Thumb3Op::Cmp,
            0b10 => Thumb3Op::Add,
            0b11 => Thumb3Op::Sub,
            _ => unreachable!(),
        };
        Some(DecodedThumb3 { op: mnemonic, rd, imm8 })
    }

    /// End to end smoke test for the bus trampoline plumbing. Builds a tiny
    /// function with signature
    ///     extern "C" fn(gpr_ptr: *mut u32, cpu_ctx: *mut u8, addr: u32)
    /// that calls the registered rba_bus_load_32 with (cpu_ctx, addr) and
    /// stores the returned value into gpr[0]. Lets a unit test verify the
    /// Rust -> Cranelift -> Rust callback round trip works without needing
    /// the full LDR decoder landed yet.
    ///
    /// Panics if this compiler was not built with `new_with_bus`.
    pub fn compile_bus_load_32_stub(
        &mut self,
    ) -> extern "C" fn(*mut u32, *mut u8, u32) {
        let imports = self
            .bus_imports
            .expect("compile_bus_load_32_stub requires new_with_bus()");

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));   // gpr_ptr
        sig.params.push(AbiParam::new(ptr_type));   // cpu_ctx
        sig.params.push(AbiParam::new(types::I32)); // addr

        self.next_id += 1;
        let name = format!("dynarec_bus_stub_{}", self.next_id);
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
            let cpu_ctx = builder.block_params(entry)[1];
            let addr = builder.block_params(entry)[2];

            // Reference the import inside this function so we can call it.
            let callee = self
                .module
                .declare_func_in_func(imports.load_32, builder.func);
            let call = builder.ins().call(callee, &[cpu_ctx, addr]);
            let loaded = builder.inst_results(call)[0];

            // gpr[0] = loaded
            builder
                .ins()
                .store(MemFlags::trusted(), loaded, gpr_ptr, Offset32::new(0));

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
        unsafe {
            std::mem::transmute::<
                *const u8,
                extern "C" fn(*mut u32, *mut u8, u32),
            >(code)
        }
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

/// A decoded ARM B or BL instruction. `offset24_signed` is the raw 24 bit
/// immediate sign extended to i32 (not yet shifted by 2).
#[derive(Clone, Copy, Debug)]
struct DecodedBranch {
    cond: ArmCond,
    link: bool,
    offset24_signed: i32,
}

/// Thumb format 3 mnemonic: MOV / CMP / ADD / SUB Rd, #imm8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thumb3Op { Mov, Cmp, Add, Sub }

#[derive(Clone, Copy, Debug)]
struct DecodedThumb3 {
    op: Thumb3Op,
    rd: i32,
    imm8: u32,
}

/// Thumb format 2: ADD/SUB Rd, Rs, (Rn | #imm3). operand picks between
/// a register source or a 3 bit immediate.
#[derive(Clone, Copy, Debug)]
enum Thumb2Operand {
    Reg(i32),
    Imm3(u32),
}

#[derive(Clone, Copy, Debug)]
struct DecodedThumb2 {
    sub: bool, // false = ADD, true = SUB
    rd: i32,
    rs: i32,
    operand: Thumb2Operand,
}

/// Thumb format 4 logical subset mnemonic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thumb4Op { And, Eor, Orr, Bic, Mvn, Tst, Cmp, Cmn }

#[derive(Clone, Copy, Debug)]
struct DecodedThumb4 {
    op: Thumb4Op,
    rd: i32,
    rs: i32,
}

/// Thumb format 5 (Hi register) mnemonic, non-branch. ADD/MOV don't
/// update flags in this form; CMP does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thumb5Op { Add, Cmp, Mov }

#[derive(Clone, Copy, Debug)]
struct DecodedThumb5 {
    op: Thumb5Op,
    rd: i32,
    rs: i32,
}

/// Thumb format 9 LDR/STR immediate offset (word or unsigned byte).
#[derive(Clone, Copy, Debug)]
struct DecodedThumb9 {
    load: bool,   // true = LDR/LDRB, false = STR/STRB
    byte: bool,   // true = byte size, false = word
    offset: u32,  // already scaled (word: imm5*4, byte: imm5)
    rs: i32,      // base register
    rd: i32,      // dest / src register
}

/// Thumb format 11 SP relative LDR/STR word.
#[derive(Clone, Copy, Debug)]
struct DecodedThumb11 {
    load: bool,
    rd: i32,
    offset: u32, // already scaled by 4
}

/// Thumb format 14 PUSH/POP register list (non PC variant).
/// `extra_reg` is LR for PUSH or PC for POP. POP with extra_reg=true is
/// rejected by the classifier because it's a block terminator handled
/// elsewhere.
#[derive(Clone, Copy, Debug)]
struct DecodedThumb14 {
    push: bool,      // true = PUSH (L=0), false = POP (L=1)
    extra_reg: bool, // R bit
    reg_list: u8,    // R0..R7 bitmap
}

impl DecodedThumb14 {
    /// Total number of registers transferred by this instruction.
    fn count(&self) -> u32 {
        self.reg_list.count_ones() + self.extra_reg as u32
    }
}

/// Thumb BX Rs (format 5 with oo=11). Reads gpr[rs] at runtime and jumps
/// there, preserving bit 0 as the ARM/Thumb mode signal.
#[derive(Clone, Copy, Debug)]
struct DecodedThumbBx {
    rs: i32,
}

/// Thumb PC relative branch (format 16 conditional or format 18
/// unconditional). Target pc is computed at codegen time from
/// entry_pc + in-block offset + 4 (pipeline) + (offset << 1).
#[derive(Clone, Copy, Debug)]
struct DecodedThumbPcBranch {
    cond: ArmCond,           // Al for format 18
    offset_signed: i32,      // already sign extended, NOT yet shifted by 1
}

/// ARM shifter operation kind. Only the three Thumb format 1 variants
/// for now. Format 4 reg shifts (LSL/LSR/ASR/ROR with register amount)
/// would extend this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShiftKind { Lsl, Lsr, Asr }

#[derive(Clone, Copy, Debug)]
struct DecodedThumb1 {
    kind: ShiftKind,
    imm5: u32,
    rs: i32,
    rd: i32,
}

/// A decoded ARM LDR / STR immediate instruction. Pre indexed, no writeback,
/// any offset sign, either word or byte size. The dynarec emits this by
/// calling into the bus trampolines at runtime.
#[derive(Clone, Copy, Debug)]
struct DecodedMem {
    cond: ArmCond,
    /// true = LDR, false = STR
    load: bool,
    /// true = byte, false = word.
    byte: bool,
    /// true = add offset to base, false = subtract.
    add: bool,
    rd: i32,
    rn: i32,
    offset: u32,
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

/// Emit a Thumb format 14 PUSH or POP register list.
///
/// Address ordering (ARMv4 PUSH/POP = STMDB/LDMIA on SP):
///   PUSH:  new_sp = SP - 4*count; write registers low-to-high to
///          addresses new_sp, new_sp+4, ... (lowest reg at lowest addr).
///          End SP = new_sp.
///   POP:   read registers low-to-high from SP, SP+4, ... up to
///          SP + 4*(count-1). End SP = SP + 4*count.
///
/// The register list for PUSH is R0..R7 ordered, then LR. For POP the
/// order is R0..R7, then PC. We only handle POP without PC here
/// (classifier rejects).
fn emit_thumb_format14(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpu_ctx: Value,
    load_32_ref: cranelift::codegen::ir::FuncRef,
    store_32_ref: cranelift::codegen::ir::FuncRef,
    dec: DecodedThumb14,
) {
    let count = dec.count();
    let bytes = (count as i64) * 4;
    let sp = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(13 * 4),
    );

    let start_addr = if dec.push {
        builder.ins().iadd_imm(sp, -bytes)
    } else {
        sp
    };

    // Walk the register list low-to-high.
    let mut byte_offset = 0i64;
    for i in 0..8 {
        if dec.reg_list & (1 << i) != 0 {
            let addr = builder.ins().iadd_imm(start_addr, byte_offset);
            if dec.push {
                let v = builder.ins().load(
                    types::I32, MemFlags::trusted(), gpr_ptr, Offset32::new(i * 4),
                );
                builder.ins().call(store_32_ref, &[cpu_ctx, addr, v]);
            } else {
                let call = builder.ins().call(load_32_ref, &[cpu_ctx, addr]);
                let v = builder.inst_results(call)[0];
                builder.ins().store(
                    MemFlags::trusted(), v, gpr_ptr, Offset32::new(i * 4),
                );
            }
            byte_offset += 4;
        }
    }
    // LR bit for PUSH (extra_reg = LR = R14). POP with PC bit was rejected
    // by the classifier.
    if dec.extra_reg && dec.push {
        let addr = builder.ins().iadd_imm(start_addr, byte_offset);
        let lr = builder.ins().load(
            types::I32, MemFlags::trusted(), gpr_ptr, Offset32::new(14 * 4),
        );
        builder.ins().call(store_32_ref, &[cpu_ctx, addr, lr]);
        // byte_offset += 4; (unused after this, kept for readability)
    }

    // Update SP.
    let new_sp = if dec.push {
        start_addr // which is sp - bytes
    } else {
        builder.ins().iadd_imm(sp, bytes)
    };
    builder.ins().store(
        MemFlags::trusted(), new_sp, gpr_ptr, Offset32::new(13 * 4),
    );
}

/// Emit a Thumb format 11 SP relative LDR/STR (word). Base register is
/// hardcoded to R13 (SP) in the encoding.
fn emit_thumb_format11(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpu_ctx: Value,
    load_32_ref: cranelift::codegen::ir::FuncRef,
    store_32_ref: cranelift::codegen::ir::FuncRef,
    dec: DecodedThumb11,
) {
    let sp = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(13 * 4),
    );
    let offset = builder.ins().iconst(types::I32, dec.offset as i64);
    let addr = builder.ins().iadd(sp, offset);
    if dec.load {
        let call = builder.ins().call(load_32_ref, &[cpu_ctx, addr]);
        let v = builder.inst_results(call)[0];
        builder.ins().store(MemFlags::trusted(), v, gpr_ptr, Offset32::new(dec.rd * 4));
    } else {
        let rd_val = builder.ins().load(
            types::I32, MemFlags::trusted(), gpr_ptr, Offset32::new(dec.rd * 4),
        );
        builder.ins().call(store_32_ref, &[cpu_ctx, addr, rd_val]);
    }
}

/// Emit a Thumb format 9 LDR/STR immediate offset. addr = gpr[rs] + offset.
/// Word form uses load_32/store_32 trampolines; byte form uses load_8/store_8
/// with zero extension (LDRB) or low-byte truncation (STRB).
fn emit_thumb_format9(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpu_ctx: Value,
    load_32_ref: cranelift::codegen::ir::FuncRef,
    store_32_ref: cranelift::codegen::ir::FuncRef,
    load_8_ref: cranelift::codegen::ir::FuncRef,
    store_8_ref: cranelift::codegen::ir::FuncRef,
    dec: DecodedThumb9,
) {
    let rs_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rs * 4),
    );
    let offset = builder.ins().iconst(types::I32, dec.offset as i64);
    let addr = builder.ins().iadd(rs_val, offset);

    match (dec.load, dec.byte) {
        (true, false) => {
            let call = builder.ins().call(load_32_ref, &[cpu_ctx, addr]);
            let v = builder.inst_results(call)[0];
            builder.ins().store(MemFlags::trusted(), v, gpr_ptr, Offset32::new(dec.rd * 4));
        }
        (true, true) => {
            let call = builder.ins().call(load_8_ref, &[cpu_ctx, addr]);
            let v = builder.inst_results(call)[0];
            let zero_ext = builder.ins().band_imm(v, 0xff);
            builder.ins().store(MemFlags::trusted(), zero_ext, gpr_ptr, Offset32::new(dec.rd * 4));
        }
        (false, false) => {
            let rd_val = builder.ins().load(
                types::I32, MemFlags::trusted(), gpr_ptr, Offset32::new(dec.rd * 4),
            );
            builder.ins().call(store_32_ref, &[cpu_ctx, addr, rd_val]);
        }
        (false, true) => {
            let rd_val = builder.ins().load(
                types::I32, MemFlags::trusted(), gpr_ptr, Offset32::new(dec.rd * 4),
            );
            let byte_val = builder.ins().band_imm(rd_val, 0xff);
            builder.ins().call(store_8_ref, &[cpu_ctx, addr, byte_val]);
        }
    }
}

/// Emit a Thumb format 5 non-branch op (ADD/CMP/MOV with Hi registers).
/// ADD and MOV do not update flags in this form. CMP updates N/Z/C/V
/// just like CMP in format 4 / ARM DP S bit.
fn emit_thumb_format5_non_branch(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedThumb5,
) {
    let rd_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rd * 4),
    );
    let rs_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rs * 4),
    );

    match dec.op {
        Thumb5Op::Mov => {
            // Rd = Rs, no flag update.
            builder.ins().store(
                MemFlags::trusted(),
                rs_val,
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
        }
        Thumb5Op::Add => {
            // Rd = Rd + Rs, no flag update.
            let result = builder.ins().iadd(rd_val, rs_val);
            builder.ins().store(
                MemFlags::trusted(),
                result,
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
        }
        Thumb5Op::Cmp => {
            // flags from Rd - Rs, no writeback. Reuse the ARM DP S bit
            // path via emit_flag_update with DpOp::Cmp.
            let result = builder.ins().isub(rd_val, rs_val);
            let new_cpsr = emit_flag_update(builder, cpsr_var, DpOp::Cmp, rd_val, rs_val, result);
            builder.def_var(cpsr_var, new_cpsr);
        }
    }
}

/// Emit a Thumb format 1 shift by immediate (LSL/LSR/ASR Rd, Rs, #imm5).
/// Writes N, Z, and C (shifter carry) to CPSR. Preserves V.
///
/// ARM7TDMI barrel shifter special cases:
///   LSL #0:  result = Rs, C preserved
///   LSR #0:  decoded as LSR #32 -> result = 0,  C = bit 31 of Rs
///   ASR #0:  decoded as ASR #32 -> result = Rs arith shift by 31 (all sign
///            bits), C = bit 31 of Rs
///   LSL #n (1..31): result = Rs << n,     C = bit (32-n) of Rs
///   LSR #n (1..31): result = Rs >> n,     C = bit (n-1) of Rs
///   ASR #n (1..31): result = (i32)Rs >> n, C = bit (n-1) of Rs
fn emit_thumb_format1(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedThumb1,
) {
    let rs_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rs * 4),
    );

    // Precompute result and shifter-out C bit at codegen time by folding
    // the constant imm5 into specific instruction sequences. This avoids
    // runtime branching on the shift amount.
    let one = builder.ins().iconst(types::I32, 1);
    let preserve_c = {
        // Current C bit from cpsr_var, shifted down to bit 0.
        let cpsr = builder.use_var(cpsr_var);
        let shifted = builder.ins().ushr_imm(cpsr, 29);
        builder.ins().band(shifted, one)
    };

    let (result, new_c) = match (dec.kind, dec.imm5) {
        (ShiftKind::Lsl, 0) => {
            // LSL #0: no shift, C preserved.
            (rs_val, preserve_c)
        }
        (ShiftKind::Lsl, n) => {
            // result = Rs << n ; C = bit (32 - n) of Rs
            let r = builder.ins().ishl_imm(rs_val, n as i64);
            let c_shift = 32 - n as i64;
            let c_raw = builder.ins().ushr_imm(rs_val, c_shift);
            let c = builder.ins().band(c_raw, one);
            (r, c)
        }
        (ShiftKind::Lsr, 0) => {
            // LSR #0 means LSR #32: result = 0, C = bit 31 of Rs.
            let r = builder.ins().iconst(types::I32, 0);
            let c_raw = builder.ins().ushr_imm(rs_val, 31);
            let c = builder.ins().band(c_raw, one);
            (r, c)
        }
        (ShiftKind::Lsr, n) => {
            let r = builder.ins().ushr_imm(rs_val, n as i64);
            let c_raw = builder.ins().ushr_imm(rs_val, (n - 1) as i64);
            let c = builder.ins().band(c_raw, one);
            (r, c)
        }
        (ShiftKind::Asr, 0) => {
            // ASR #0 means ASR #32: result = all sign bits, C = bit 31.
            let r = builder.ins().sshr_imm(rs_val, 31);
            let c_raw = builder.ins().ushr_imm(rs_val, 31);
            let c = builder.ins().band(c_raw, one);
            (r, c)
        }
        (ShiftKind::Asr, n) => {
            let r = builder.ins().sshr_imm(rs_val, n as i64);
            let c_raw = builder.ins().ushr_imm(rs_val, (n - 1) as i64);
            let c = builder.ins().band(c_raw, one);
            (r, c)
        }
    };

    builder.ins().store(
        MemFlags::trusted(),
        result,
        gpr_ptr,
        Offset32::new(dec.rd * 4),
    );

    // N, Z from result. C is new_c. V preserved.
    let zero = builder.ins().iconst(types::I32, 0);
    let n = builder.ins().ushr_imm(result, 31);
    let z_bool = builder.ins().icmp(IntCC::Equal, result, zero);
    let z = builder.ins().uextend(types::I32, z_bool);
    let v = {
        let cpsr = builder.use_var(cpsr_var);
        let shifted = builder.ins().ushr_imm(cpsr, 28);
        builder.ins().band(shifted, one)
    };

    let cpsr = builder.use_var(cpsr_var);
    let mask = builder.ins().iconst(types::I32, 0x0fff_ffff);
    let cleared = builder.ins().band(cpsr, mask);
    let n_shifted = builder.ins().ishl_imm(n, 31);
    let z_shifted = builder.ins().ishl_imm(z, 30);
    let c_shifted = builder.ins().ishl_imm(new_c, 29);
    let v_shifted = builder.ins().ishl_imm(v, 28);
    let nz = builder.ins().bor(n_shifted, z_shifted);
    let cv = builder.ins().bor(c_shifted, v_shifted);
    let flags = builder.ins().bor(nz, cv);
    let new_cpsr = builder.ins().bor(cleared, flags);
    builder.def_var(cpsr_var, new_cpsr);
}

/// Emit a Thumb format 4 logical / compare op. All mnemonics in this
/// subset update NZ flags. AND/EOR/ORR/BIC/MVN writeback to Rd; TST/CMP/CMN
/// do not. C and V behave like the equivalent ARM DP S bit path:
/// preserved for logical, computed for CMP/CMN.
fn emit_thumb_format4_logical(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedThumb4,
) {
    let rd_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rd * 4),
    );
    let rs_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rs * 4),
    );

    let (result, dp_equivalent, writeback) = match dec.op {
        Thumb4Op::And => (builder.ins().band(rd_val, rs_val), DpOp::Tst, true),
        Thumb4Op::Eor => (builder.ins().bxor(rd_val, rs_val), DpOp::Teq, true),
        Thumb4Op::Orr => {
            // ARM ORR flag semantics == TST (logical, N/Z from result,
            // preserve C/V). The Tst branch of emit_flag_update reads the
            // existing cpsr C/V and leaves them. Same goes for EOR vs TEQ.
            let r = builder.ins().bor(rd_val, rs_val);
            (r, DpOp::Tst, true)
        }
        Thumb4Op::Bic => {
            // Rd = Rd & ~Rs
            let not_rs = builder.ins().bnot(rs_val);
            let r = builder.ins().band(rd_val, not_rs);
            (r, DpOp::Tst, true)
        }
        Thumb4Op::Mvn => {
            // Rd = ~Rs  (Rd value ignored as source).
            let r = builder.ins().bnot(rs_val);
            (r, DpOp::Mov, true)
        }
        Thumb4Op::Tst => (builder.ins().band(rd_val, rs_val), DpOp::Tst, false),
        Thumb4Op::Cmp => (builder.ins().isub(rd_val, rs_val), DpOp::Cmp, false),
        Thumb4Op::Cmn => (builder.ins().iadd(rd_val, rs_val), DpOp::Cmn, false),
    };

    if writeback {
        builder.ins().store(
            MemFlags::trusted(),
            result,
            gpr_ptr,
            Offset32::new(dec.rd * 4),
        );
    }

    let new_cpsr = emit_flag_update(builder, cpsr_var, dp_equivalent, rd_val, rs_val, result);
    builder.def_var(cpsr_var, new_cpsr);
}

/// Emit a Thumb format 2 add/sub. Always updates full NZCV (Thumb
/// ADD/SUB behave like ARM DP with S=1 always in ARMv4 outside IT blocks).
fn emit_thumb_format2(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedThumb2,
) {
    let rs_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rs * 4),
    );
    let rhs = match dec.operand {
        Thumb2Operand::Imm3(v) => builder.ins().iconst(types::I32, v as i64),
        Thumb2Operand::Reg(rn) => builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            gpr_ptr,
            Offset32::new(rn * 4),
        ),
    };
    let (result, dp_equivalent) = if dec.sub {
        (builder.ins().isub(rs_val, rhs), DpOp::Sub)
    } else {
        (builder.ins().iadd(rs_val, rhs), DpOp::Add)
    };
    builder.ins().store(
        MemFlags::trusted(),
        result,
        gpr_ptr,
        Offset32::new(dec.rd * 4),
    );
    let new_cpsr = emit_flag_update(builder, cpsr_var, dp_equivalent, rs_val, rhs, result);
    builder.def_var(cpsr_var, new_cpsr);
}

/// Emit a Thumb format 3 immediate8 instruction. Always updates NZCV.
/// MOV: Rd = imm8, sets N=0 (imm8 < 0x80 always clears top bit),
///      Z=(imm8==0), preserves C and V.
/// CMP: flags from Rd - imm8. No writeback.
/// ADD: Rd = Rd + imm8. Full NZCV.
/// SUB: Rd = Rd - imm8. Full NZCV.
fn emit_thumb_format3(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    dec: DecodedThumb3,
) {
    let imm8 = builder.ins().iconst(types::I32, dec.imm8 as i64);
    let rd_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rd * 4),
    );

    let (result, dp_equivalent, writeback) = match dec.op {
        Thumb3Op::Mov => (imm8, DpOp::Mov, true),
        Thumb3Op::Cmp => {
            let r = builder.ins().isub(rd_val, imm8);
            (r, DpOp::Cmp, false)
        }
        Thumb3Op::Add => {
            let r = builder.ins().iadd(rd_val, imm8);
            (r, DpOp::Add, true)
        }
        Thumb3Op::Sub => {
            let r = builder.ins().isub(rd_val, imm8);
            (r, DpOp::Sub, true)
        }
    };

    if writeback {
        builder.ins().store(
            MemFlags::trusted(),
            result,
            gpr_ptr,
            Offset32::new(dec.rd * 4),
        );
    }

    // Flag update mirrors the ARM DP S bit path. For Thumb MOV imm8 the
    // DpOp::Mov branch of emit_flag_update preserves C and V which matches
    // ARMv4 Thumb semantics. ADD/SUB/CMP compute the right NZCV in that
    // same function.
    // Use the pre writeback rd value as the "rn" operand for flag calc.
    // For MOV imm8, rn is unused by the flag path (DpOp::Mov only reads
    // result for N/Z).
    let new_cpsr = emit_flag_update(builder, cpsr_var, dp_equivalent, rd_val, imm8, result);
    builder.def_var(cpsr_var, new_cpsr);
}

/// Emit a conditionally executed LDR / STR immediate. Loads rn, adjusts by
/// the signed offset, then calls the right size trampoline (load_32 /
/// store_32 for word, load_8 / store_8 for byte). For LDR the returned value
/// is stored into gpr[rd]; for STR the value from gpr[rd] is passed out.
fn emit_conditional_mem(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpsr_var: Variable,
    cpu_ctx: Value,
    load_32_ref: cranelift::codegen::ir::FuncRef,
    store_32_ref: cranelift::codegen::ir::FuncRef,
    load_8_ref: cranelift::codegen::ir::FuncRef,
    store_8_ref: cranelift::codegen::ir::FuncRef,
    dec: DecodedMem,
) {
    if dec.cond == ArmCond::Al {
        emit_mem_body(
            builder, gpr_ptr, cpu_ctx,
            load_32_ref, store_32_ref, load_8_ref, store_8_ref,
            dec,
        );
        return;
    }

    let cond_result = emit_cond_check(builder, cpsr_var, dec.cond);
    let body = builder.create_block();
    let merge = builder.create_block();
    builder.ins().brif(cond_result, body, &[], merge, &[]);
    builder.switch_to_block(body);
    builder.seal_block(body);
    emit_mem_body(
        builder, gpr_ptr, cpu_ctx,
        load_32_ref, store_32_ref, load_8_ref, store_8_ref,
        dec,
    );
    builder.ins().jump(merge, &[]);
    builder.switch_to_block(merge);
    builder.seal_block(merge);
}

fn emit_mem_body(
    builder: &mut FunctionBuilder,
    gpr_ptr: Value,
    cpu_ctx: Value,
    load_32_ref: cranelift::codegen::ir::FuncRef,
    store_32_ref: cranelift::codegen::ir::FuncRef,
    load_8_ref: cranelift::codegen::ir::FuncRef,
    store_8_ref: cranelift::codegen::ir::FuncRef,
    dec: DecodedMem,
) {
    // addr = gpr[rn] +/- imm12  (U bit from encoding picks add vs sub).
    let rn_val = builder.ins().load(
        types::I32,
        MemFlags::trusted(),
        gpr_ptr,
        Offset32::new(dec.rn * 4),
    );
    let offset = builder.ins().iconst(types::I32, dec.offset as i64);
    let addr = if dec.add {
        builder.ins().iadd(rn_val, offset)
    } else {
        builder.ins().isub(rn_val, offset)
    };

    match (dec.load, dec.byte) {
        (true, false) => {
            let call = builder.ins().call(load_32_ref, &[cpu_ctx, addr]);
            let loaded = builder.inst_results(call)[0];
            builder.ins().store(
                MemFlags::trusted(),
                loaded,
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
        }
        (true, true) => {
            // LDRB returns u32 zero extended via the u32 sig on the
            // trampoline. Mask to 8 bits on the trampoline side; here we
            // just store what comes back.
            let call = builder.ins().call(load_8_ref, &[cpu_ctx, addr]);
            let loaded = builder.inst_results(call)[0];
            let byte_only = builder.ins().band_imm(loaded, 0xff);
            builder.ins().store(
                MemFlags::trusted(),
                byte_only,
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
        }
        (false, false) => {
            let rd_val = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
            builder.ins().call(store_32_ref, &[cpu_ctx, addr, rd_val]);
        }
        (false, true) => {
            let rd_val = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                gpr_ptr,
                Offset32::new(dec.rd * 4),
            );
            // STRB writes only the low byte. Trampoline is responsible for
            // truncation; we mask here too so the contract is explicit.
            let byte_val = builder.ins().band_imm(rd_val, 0xff);
            builder.ins().call(store_8_ref, &[cpu_ctx, addr, byte_val]);
        }
    }
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

    #[test]
    fn decode_unconditional_b_forward() {
        // B #+8 (skip the next instruction). ARM encoding:
        //   cond=AL, opcode 1010, imm24 = 0 means target = pc+8+0 = pc+8.
        // E.g. "EA FF FF FE" is B -8 (infinite loop); for a +8 jump use
        // imm24 = 0.
        //   EA 00 00 00  = B pc+8 (= next+4, i.e. skip one instr)
        let insn = 0xEA00_0000u32;
        let dec = DynarecCompiler::decode_branch(insn)
            .expect("should be a branch");
        assert_eq!(dec.cond, ArmCond::Al);
        assert_eq!(dec.link, false);
        assert_eq!(dec.offset24_signed, 0);

        // BL #-8  cond=AL, opcode 1011, imm24 = 0x_FF_FF_FE (sign-ext'd to
        // -2), target = pc+8 + (-2)*4 = pc.
        let insn = 0xEBFF_FFFEu32;
        let dec = DynarecCompiler::decode_branch(insn)
            .expect("should be a branch");
        assert_eq!(dec.link, true);
        assert_eq!(dec.offset24_signed, -2);
    }

    #[test]
    fn decode_branch_rejects_non_branch() {
        // MOV R0, #5 isn't a branch.
        assert!(DynarecCompiler::decode_branch(0xE3A0_0005u32).is_none());
        // NV (0xF) cond reserved / never-taken, reject.
        assert!(DynarecCompiler::decode_branch(0xFA00_0000u32).is_none());
    }

    #[test]
    fn compile_b_forward_taken() {
        // Block: MOV R0, #1; B +8. Both always taken (AL cond).
        let mut compiler = DynarecCompiler::new();
        let mov = 0xE3A0_0001u32;        // MOV R0, #1
        let b_plus8 = 0xEA00_0000u32;    // B +8 (target pc+8 from branch site)
        let entry_pc: u32 = 0x0800_1000;
        let func = compiler
            .try_compile_block_with_branch(&[mov, b_plus8], entry_pc)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out: u32 = 0xDEAD_BEEF;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);

        assert_eq!(gpr[0], 1, "MOV ran");
        assert_eq!(taken, 1, "branch should report taken");
        // B at entry_pc+4. Target = (entry_pc+4) + 8 + 0 = entry_pc + 12.
        assert_eq!(pc_out, entry_pc.wrapping_add(12));
    }

    #[test]
    fn compile_bl_sets_lr() {
        // BL +8 -> LR = pc_of_BL + 4, target = pc_of_BL + 8.
        let mut compiler = DynarecCompiler::new();
        let bl = 0xEB00_0000u32;
        let entry_pc: u32 = 0x0800_2000;
        let func = compiler
            .try_compile_block_with_branch(&[bl], entry_pc)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);

        assert_eq!(taken, 1);
        assert_eq!(gpr[14], entry_pc.wrapping_add(4), "LR = PC_of_BL + 4");
        assert_eq!(pc_out, entry_pc.wrapping_add(8));
    }

    #[test]
    fn compile_conditional_branch_not_taken() {
        // BEQ +8 with Z=0 (not equal) -> branch not taken, block falls
        // through.
        let mut compiler = DynarecCompiler::new();
        let beq = 0x0A00_0000u32; // cond=EQ, B opcode, imm24=0
        let func = compiler
            .try_compile_block_with_branch(&[beq], 0x0800_3000)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32; // Z=0
        let mut pc_out: u32 = 0xBADD_F00D;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);

        assert_eq!(taken, 0, "BEQ should not fire when Z=0");
        // pc_out must be left alone.
        assert_eq!(pc_out, 0xBADD_F00D);
    }

    #[test]
    fn compile_conditional_branch_taken_when_z_set() {
        let mut compiler = DynarecCompiler::new();
        let beq = 0x0A00_0000u32;
        let entry_pc: u32 = 0x0800_4000;
        let func = compiler
            .try_compile_block_with_branch(&[beq], entry_pc)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr: u32 = 1 << 30; // Z=1
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);

        assert_eq!(taken, 1);
        assert_eq!(pc_out, entry_pc.wrapping_add(8));
    }

    #[test]
    fn branch_body_rejects_unsupported_midblock() {
        // Body contains an unsupported opcode (LDR): should be None.
        let ldr_placeholder = 0xE5101000u32; // LDR R1, [R0]
        let b = 0xEA00_0000u32;
        let mut compiler = DynarecCompiler::new();
        assert!(
            compiler
                .try_compile_block_with_branch(&[ldr_placeholder, b], 0)
                .is_none()
        );
    }

    // --- Bus trampoline plumbing ---
    //
    // These tests exercise the Rust -> Cranelift -> Rust round trip used by
    // the upcoming LDR/STR codegen without decoding any ARM memory ops yet.
    // They depend on a test-only trampoline pair plus a tiny 256 byte fake
    // memory buffer.

    use std::cell::UnsafeCell;

    /// Fake memory used by the bus trampoline tests. `UnsafeCell` so the
    /// `extern "C"` callbacks can mutate it through a raw pointer without
    /// tripping the borrow checker. Safe in tests because each test runs
    /// single threaded and constructs its own buffer.
    struct TestBus {
        bytes: UnsafeCell<[u8; 256]>,
    }

    unsafe extern "C" fn test_load_32(ctx: *mut u8, addr: u32) -> u32 {
        let bus = &*(ctx as *const TestBus);
        let bytes = &*bus.bytes.get();
        let a = addr as usize & 0xFC;
        u32::from_le_bytes([bytes[a], bytes[a + 1], bytes[a + 2], bytes[a + 3]])
    }
    unsafe extern "C" fn test_store_32(ctx: *mut u8, addr: u32, val: u32) {
        let bus = &*(ctx as *const TestBus);
        let bytes = &mut *bus.bytes.get();
        let a = addr as usize & 0xFC;
        let v = val.to_le_bytes();
        bytes[a] = v[0]; bytes[a + 1] = v[1]; bytes[a + 2] = v[2]; bytes[a + 3] = v[3];
    }
    unsafe extern "C" fn test_load_8(ctx: *mut u8, addr: u32) -> u32 {
        let bus = &*(ctx as *const TestBus);
        let bytes = &*bus.bytes.get();
        bytes[addr as usize & 0xFF] as u32
    }
    unsafe extern "C" fn test_store_8(ctx: *mut u8, addr: u32, val: u32) {
        let bus = &*(ctx as *const TestBus);
        let bytes = &mut *bus.bytes.get();
        bytes[addr as usize & 0xFF] = val as u8;
    }

    fn test_trampolines() -> BusTrampolines {
        BusTrampolines {
            load_32: test_load_32,
            store_32: test_store_32,
            load_8: test_load_8,
            store_8: test_store_8,
        }
    }

    #[test]
    fn bus_stub_round_trip_load_32() {
        let bus = TestBus { bytes: UnsafeCell::new([0u8; 256]) };
        // Seed a known value at offset 0x10.
        unsafe {
            let bytes = &mut *bus.bytes.get();
            bytes[0x10] = 0x11;
            bytes[0x11] = 0x22;
            bytes[0x12] = 0x33;
            bytes[0x13] = 0x44;
        }

        let mut compiler = DynarecCompiler::new_with_bus(test_trampolines());
        assert!(compiler.has_bus());
        let stub = compiler.compile_bus_load_32_stub();

        let mut gpr = [0u32; 15];
        stub(gpr.as_mut_ptr(), &bus as *const TestBus as *mut u8, 0x10);
        assert_eq!(gpr[0], 0x44332211, "round trip through bus trampoline");
    }

    #[test]
    fn plain_compiler_has_no_bus() {
        let c = DynarecCompiler::new();
        assert!(!c.has_bus());
    }

    // --- ARM LDR / STR immediate codegen ---

    fn new_bus_and_compiler() -> (TestBus, DynarecCompiler) {
        (
            TestBus { bytes: UnsafeCell::new([0u8; 256]) },
            DynarecCompiler::new_with_bus(test_trampolines()),
        )
    }

    #[test]
    fn decode_ldr_str_immediate_shapes() {
        // LDR R1, [R0, #4]  ->  E5_90_10_04
        let ldr = 0xE590_1004u32;
        let d = DynarecCompiler::decode_mem_immediate(ldr).expect("LDR");
        assert_eq!(d.cond, ArmCond::Al);
        assert_eq!(d.load, true);
        assert_eq!(d.byte, false);
        assert_eq!(d.add, true);
        assert_eq!(d.rn, 0);
        assert_eq!(d.rd, 1);
        assert_eq!(d.offset, 4);

        // STR R2, [R3, #0x20]  ->  E5_83_20_20
        let str_ = 0xE583_2020u32;
        let d = DynarecCompiler::decode_mem_immediate(str_).expect("STR");
        assert_eq!(d.load, false);
        assert_eq!(d.byte, false);
        assert_eq!(d.rn, 3);
        assert_eq!(d.rd, 2);

        // LDRB R1, [R0, #4]  B=1  ->  E5_D0_10_04
        let ldrb = 0xE5D0_1004u32;
        let d = DynarecCompiler::decode_mem_immediate(ldrb).expect("LDRB");
        assert_eq!(d.load, true);
        assert_eq!(d.byte, true);
        assert_eq!(d.offset, 4);

        // STRB R1, [R0, #4]  B=1,L=0  ->  E5_C0_10_04
        let strb = 0xE5C0_1004u32;
        let d = DynarecCompiler::decode_mem_immediate(strb).expect("STRB");
        assert_eq!(d.load, false);
        assert_eq!(d.byte, true);

        // LDR with negative offset (U=0) accepted now.
        //   E5_10_10_04
        let ldr_neg = 0xE510_1004u32;
        let d = DynarecCompiler::decode_mem_immediate(ldr_neg).expect("LDR -ve");
        assert_eq!(d.add, false);
        assert_eq!(d.offset, 4);

        // Writeback (W=1) still rejected.
        let ldr_wb = 0xE5B0_1004u32;
        assert!(DynarecCompiler::decode_mem_immediate(ldr_wb).is_none());

        // Post indexed (P=0) still rejected.
        let ldr_post = 0xE490_1004u32;
        assert!(DynarecCompiler::decode_mem_immediate(ldr_post).is_none());
    }

    #[test]
    fn compile_ldrb_reads_byte_zero_extended() {
        let (bus, mut compiler) = new_bus_and_compiler();
        unsafe {
            let b = &mut *bus.bytes.get();
            b[0x40] = 0xAB;
        }
        // LDRB R1, [R0, #0x40]
        let ldrb = 0xE5D0_1040u32;
        let func = compiler
            .try_compile_mem_block(&[ldrb])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[1] = 0xDEADBEEF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0x0000_00ABu32, "LDRB zero extends");
    }

    #[test]
    fn compile_strb_truncates_to_byte() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // STRB R2, [R0, #0x50]
        let strb = 0xE5C0_2050u32;
        let func = compiler
            .try_compile_mem_block(&[strb])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        gpr[2] = 0x1234_5678;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        unsafe {
            let b = &*bus.bytes.get();
            assert_eq!(b[0x50], 0x78, "low byte only");
            assert_eq!(b[0x51], 0, "neighbour untouched");
        }
    }

    // --- Thumb format 3 (MOV/CMP/ADD/SUB Rd, #imm8) ---

    #[test]
    fn decode_thumb_format3_shapes() {
        // MOV R0, #0x42  -> 0010 0 000 0100 0010 = 0x2042
        let d = DynarecCompiler::decode_thumb_format3(0x2042).expect("MOV");
        assert_eq!(d.op, Thumb3Op::Mov);
        assert_eq!(d.rd, 0);
        assert_eq!(d.imm8, 0x42);

        // CMP R3, #0x10 -> 0010 1 011 0001 0000 = 0x2B10
        let d = DynarecCompiler::decode_thumb_format3(0x2B10).expect("CMP");
        assert_eq!(d.op, Thumb3Op::Cmp);
        assert_eq!(d.rd, 3);
        assert_eq!(d.imm8, 0x10);

        // ADD R5, #1 -> 0011 0 101 0000 0001 = 0x3501
        let d = DynarecCompiler::decode_thumb_format3(0x3501).expect("ADD");
        assert_eq!(d.op, Thumb3Op::Add);
        assert_eq!(d.rd, 5);

        // SUB R1, #5 -> 0011 1 001 0000 0101 = 0x3905
        let d = DynarecCompiler::decode_thumb_format3(0x3905).expect("SUB");
        assert_eq!(d.op, Thumb3Op::Sub);

        // Not format 3 (top 3 bits != 001)
        assert!(DynarecCompiler::decode_thumb_format3(0x4000).is_none());
        assert!(DynarecCompiler::decode_thumb_format3(0xE000).is_none());
    }

    #[test]
    fn compile_thumb_mov_imm8() {
        let mut compiler = DynarecCompiler::new();
        let mov_r1_42 = 0x2142u16; // MOV R1, #0x42
        let func = compiler
            .try_compile_thumb_block(&[mov_r1_42])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[1], 0x42);
        // Z flag clear (result nonzero), N clear.
        assert_eq!(cpsr & (1 << 30), 0);
        assert_eq!(cpsr & (1 << 31), 0);
    }

    #[test]
    fn compile_thumb_mov_imm8_zero_sets_z() {
        let mut compiler = DynarecCompiler::new();
        let mov_r2_0 = 0x2200u16;
        let func = compiler.try_compile_thumb_block(&[mov_r2_0]).unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[2], 0);
        assert_ne!(cpsr & (1 << 30), 0, "Z set for 0");
    }

    #[test]
    fn compile_thumb_cmp_sets_flags_no_writeback() {
        let mut compiler = DynarecCompiler::new();
        // CMP R0, #5
        let cmp = 0x2805u16;
        let func = compiler.try_compile_thumb_block(&[cmp]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[0] = 5;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 5, "CMP must not writeback");
        assert_ne!(cpsr & (1 << 30), 0, "Z should be set (5 - 5 == 0)");
    }

    #[test]
    fn compile_thumb_add_sub_sequence() {
        let mut compiler = DynarecCompiler::new();
        // MOV R0, #10; ADD R0, #5; SUB R0, #3
        let mov = 0x200Au16;
        let add = 0x3005u16;
        let sub = 0x3803u16;
        let func = compiler.try_compile_thumb_block(&[mov, add, sub]).unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 12);
        // Result nonzero, nonnegative -> Z=0, N=0.
        assert_eq!(cpsr & (1 << 30), 0);
        assert_eq!(cpsr & (1 << 31), 0);
    }

    #[test]
    fn compile_thumb_reject_unsupported() {
        let mut compiler = DynarecCompiler::new();
        // 0x4400 is Thumb format 5 (ADD Hi reg) - not in any currently
        // supported shape.
        assert!(compiler.try_compile_thumb_block(&[0x4400]).is_none());
    }

    // --- Thumb format 2 (ADD/SUB Rd, Rs, Rn|imm3) ---

    #[test]
    fn decode_thumb_format2_shapes() {
        // ADD R0, R1, R2  -> 00011_0_0_010_001_000 = 0b0001_1000_1000_1000 = 0x1888
        let d = DynarecCompiler::decode_thumb_format2(0x1888).expect("ADD reg");
        assert_eq!(d.sub, false);
        assert_eq!(d.rd, 0);
        assert_eq!(d.rs, 1);
        matches!(d.operand, Thumb2Operand::Reg(2));

        // SUB R3, R4, R5 -> 00011_0_1_101_100_011 = 0b0001_1011_0110_0011 = 0x1B63
        let d = DynarecCompiler::decode_thumb_format2(0x1B63).expect("SUB reg");
        assert_eq!(d.sub, true);
        assert_eq!(d.rd, 3);
        assert_eq!(d.rs, 4);
        matches!(d.operand, Thumb2Operand::Reg(5));

        // ADD R0, R1, #7 -> 00011_1_0_111_001_000 = 0b0001_1101_1100_1000 = 0x1DC8
        let d = DynarecCompiler::decode_thumb_format2(0x1DC8).expect("ADD imm3");
        assert_eq!(d.sub, false);
        matches!(d.operand, Thumb2Operand::Imm3(7));

        // SUB R2, R2, #1 -> 00011_1_1_001_010_010 = 0b0001_1111_0101_0010 = 0x1F52
        let d = DynarecCompiler::decode_thumb_format2(0x1F52).expect("SUB imm3");
        assert_eq!(d.sub, true);
        matches!(d.operand, Thumb2Operand::Imm3(1));

        // Not format 2 (top 5 bits != 00011)
        assert!(DynarecCompiler::decode_thumb_format2(0x2000).is_none()); // format 3
        assert!(DynarecCompiler::decode_thumb_format2(0x4000).is_none()); // format 4
    }

    #[test]
    fn compile_thumb_add_reg() {
        let mut compiler = DynarecCompiler::new();
        // ADD R0, R1, R2
        let add_reg = 0x1888u16;
        let func = compiler.try_compile_thumb_block(&[add_reg]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 10;
        gpr[2] = 20;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 30);
        assert_eq!(cpsr & (1 << 30), 0);  // Z clear
    }

    #[test]
    fn compile_thumb_sub_imm3_sets_z_on_zero() {
        let mut compiler = DynarecCompiler::new();
        // SUB R0, R0, #3 where R0 = 3
        let sub = 0x1EC0u16; // 00011_11_011_000_000 = 0b0001_1110_1100_0000 = 0x1EC0
        let func = compiler.try_compile_thumb_block(&[sub]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[0] = 3;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0);
        assert_ne!(cpsr & (1 << 30), 0, "Z should be set");
    }

    // --- Thumb format 14 PUSH/POP (non PC variant) ---

    #[test]
    fn decode_thumb_format14_shapes() {
        // PUSH {R0} -> 1011_0_10_0_00000001 = 0b1011_0100_0000_0001 = 0xB401
        let d = DynarecCompiler::decode_thumb_format14_non_pc(0xB401).expect("PUSH R0");
        assert_eq!(d.push, true);
        assert_eq!(d.extra_reg, false);
        assert_eq!(d.reg_list, 0x01);

        // PUSH {R0-R3, LR} -> 1011_0_10_1_00001111 = 0xB50F
        let d = DynarecCompiler::decode_thumb_format14_non_pc(0xB50F).expect("PUSH r,lr");
        assert_eq!(d.push, true);
        assert_eq!(d.extra_reg, true);
        assert_eq!(d.reg_list, 0x0F);
        assert_eq!(d.count(), 5);

        // POP {R4-R7} -> 1011_1_10_0_11110000 = 0xBCF0
        let d = DynarecCompiler::decode_thumb_format14_non_pc(0xBCF0).expect("POP");
        assert_eq!(d.push, false);
        assert_eq!(d.reg_list, 0xF0);

        // POP {R4-R7, PC} must be rejected here (deferred to branch path).
        //   1011_1_10_1_11110000 = 0xBDF0
        assert!(DynarecCompiler::decode_thumb_format14_non_pc(0xBDF0).is_none());

        // Empty list rejection: PUSH {} with R=0.
        //   1011_0_10_0_00000000 = 0xB400
        assert!(DynarecCompiler::decode_thumb_format14_non_pc(0xB400).is_none());

        // Not format 14.
        assert!(DynarecCompiler::decode_thumb_format14_non_pc(0x6000).is_none());
    }

    #[test]
    fn compile_thumb_push_single_reg() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // PUSH {R0}
        let push = 0xB401u16;
        let func = compiler.try_compile_thumb_mem_block(&[push]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[13] = 0x40; // SP
        gpr[0]  = 0x1234_5678;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[13], 0x40 - 4, "SP decremented by 4");
        unsafe {
            let b = &*bus.bytes.get();
            assert_eq!(u32::from_le_bytes([b[0x3C], b[0x3D], b[0x3E], b[0x3F]]),
                       0x1234_5678);
        }
    }

    #[test]
    fn compile_thumb_push_multiple_with_lr() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // PUSH {R0, R2, LR}
        //   1011_0_10_1_00000101 = 0xB505
        let push = 0xB505u16;
        let func = compiler.try_compile_thumb_mem_block(&[push]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[13] = 0x80;
        gpr[0]  = 0xAA;
        gpr[2]  = 0xBB;
        gpr[14] = 0xCC;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[13], 0x80 - 12, "SP -= 12 for 3 regs");
        unsafe {
            let b = &*bus.bytes.get();
            // R0 at lowest, then R2, then LR
            assert_eq!(b[0x74], 0xAA, "R0 at SP-12");
            assert_eq!(b[0x78], 0xBB, "R2 at SP-8");
            assert_eq!(b[0x7C], 0xCC, "LR at SP-4");
        }
    }

    #[test]
    fn compile_thumb_push_then_pop_roundtrip() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // PUSH {R0, R1}; POP {R2, R3}
        //   1011_0_10_0_00000011 = 0xB403
        //   1011_1_10_0_00001100 = 0xBC0C
        let push = 0xB403u16;
        let pop  = 0xBC0Cu16;
        let func = compiler.try_compile_thumb_mem_block(&[push, pop]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[13] = 0x60;
        gpr[0]  = 0xDEAD;
        gpr[1]  = 0xBEEF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[2], 0xDEAD, "R2 = pushed R0");
        assert_eq!(gpr[3], 0xBEEF, "R3 = pushed R1");
        assert_eq!(gpr[13], 0x60, "SP restored");
    }

    // --- Thumb format 11 SP-relative LDR/STR ---

    #[test]
    fn decode_thumb_format11_shapes() {
        // STR R0, [SP, #4]  imm8=1 scales to 4
        //   1001_0_000_00000001 = 0b1001_0000_0000_0001 = 0x9001
        let d = DynarecCompiler::decode_thumb_format11(0x9001).expect("STR");
        assert_eq!(d.load, false);
        assert_eq!(d.rd, 0);
        assert_eq!(d.offset, 4);

        // LDR R3, [SP, #0x100]  imm8=0x40 scales to 0x100
        //   1001_1_011_01000000 = 0b1001_1011_0100_0000 = 0x9B40
        let d = DynarecCompiler::decode_thumb_format11(0x9B40).expect("LDR");
        assert_eq!(d.load, true);
        assert_eq!(d.rd, 3);
        assert_eq!(d.offset, 0x100);

        // Not format 11.
        assert!(DynarecCompiler::decode_thumb_format11(0x6000).is_none());
    }

    #[test]
    fn compile_thumb_sp_relative_store_then_load() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // Simulated stack base at offset 0x40 in the 256 byte bus buffer.
        // STR R1, [SP, #0]; LDR R2, [SP, #0]
        let str_ = 0x9100u16; // 1001_0_001_00000000
        let ldr  = 0x9A00u16; // 1001_1_010_00000000
        let func = compiler
            .try_compile_thumb_mem_block(&[str_, ldr])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[13] = 0x40;
        gpr[1] = 0xCAFE_BABE;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[2], 0xCAFE_BABE);
        unsafe {
            let b = &*bus.bytes.get();
            assert_eq!(u32::from_le_bytes([b[0x40], b[0x41], b[0x42], b[0x43]]),
                       0xCAFE_BABE);
        }
    }

    #[test]
    fn compile_thumb_sp_relative_with_offset() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // STR R0, [SP, #8]; LDR R1, [SP, #8]
        let str_ = 0x9002u16; // imm8=2 -> offset 8
        let ldr  = 0x9902u16;
        let func = compiler
            .try_compile_thumb_mem_block(&[str_, ldr])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[13] = 0x30;
        gpr[0]  = 0x1234_5678;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0x1234_5678);
    }

    // --- Thumb format 9 LDR/STR immediate offset ---

    #[test]
    fn decode_thumb_format9_shapes() {
        // STR R0, [R1, #4]  word, offset imm5 = 1 -> scaled to 4
        //   011_0_0_00001_001_000 = 0b0110_0000_0100_1000 = 0x6048
        let d = DynarecCompiler::decode_thumb_format9(0x6048).expect("STR word");
        assert_eq!(d.load, false);
        assert_eq!(d.byte, false);
        assert_eq!(d.offset, 4);
        assert_eq!(d.rs, 1);
        assert_eq!(d.rd, 0);

        // LDR R2, [R3, #0x1C]  imm5 = 7 -> scaled to 28
        //   011_0_1_00111_011_010 = 0b0110_1001_1101_1010 = 0x69DA
        let d = DynarecCompiler::decode_thumb_format9(0x69DA).expect("LDR word");
        assert_eq!(d.load, true);
        assert_eq!(d.offset, 28);

        // STRB R0, [R1, #3] byte offset 3
        //   011_1_0_00011_001_000 = 0b0111_0000_1100_1000 = 0x70C8
        let d = DynarecCompiler::decode_thumb_format9(0x70C8).expect("STRB");
        assert_eq!(d.byte, true);
        assert_eq!(d.offset, 3);

        // LDRB R2, [R3, #1]  011_1_1_00001_011_010 = 0b0111_1000_0101_1010 = 0x785A
        let d = DynarecCompiler::decode_thumb_format9(0x785A).expect("LDRB");
        assert_eq!(d.load, true);
        assert_eq!(d.byte, true);

        // Not format 9 (top 3 bits != 011)
        assert!(DynarecCompiler::decode_thumb_format9(0x2000).is_none());
    }

    #[test]
    fn compile_thumb_ldr_word_immediate() {
        let (bus, mut compiler) = new_bus_and_compiler();
        unsafe {
            let b = &mut *bus.bytes.get();
            b[0x14] = 0x11; b[0x15] = 0x22; b[0x16] = 0x33; b[0x17] = 0x44;
        }
        // LDR R1, [R0, #0x14] -> imm5 = 5
        //   011_0_1_00101_000_001 = 0b0110_1001_0100_0001 = 0x6941
        let ldr = 0x6941u16;
        let func = compiler
            .try_compile_thumb_mem_block(&[ldr])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0x4433_2211);
    }

    #[test]
    fn compile_thumb_strb_truncates() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // STRB R2, [R0, #5]  byte offset 5
        //   011_1_0_00101_000_010 = 0b0111_0001_0100_0010 = 0x7142
        let strb = 0x7142u16;
        let func = compiler.try_compile_thumb_mem_block(&[strb]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        gpr[2] = 0xDEAD_BEEF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        unsafe {
            let b = &*bus.bytes.get();
            assert_eq!(b[5], 0xEF);
            assert_eq!(b[6], 0, "neighbour untouched");
        }
    }

    #[test]
    fn compile_thumb_mem_mixes_with_dp() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // MOV R2, #0x42 ; STR R2, [R0, #8] ; LDR R3, [R0, #8] ; ADD R4, R3, #1
        let mov = 0x2242u16;
        // STR word offset=8 means imm5=2  011_0_0_00010_000_010 = 0x6082
        let str_ = 0x6082u16;
        // LDR word offset=8 imm5=2        011_0_1_00010_000_011 = 0x6883
        let ldr = 0x6883u16;
        // ADD R4, R3, #1 format 2 imm3    0001_1_1_0_001_011_100 = 0x1C5C
        let add = 0x1C5Cu16;
        let func = compiler
            .try_compile_thumb_mem_block(&[mov, str_, ldr, add])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[2], 0x42);
        assert_eq!(gpr[3], 0x42);
        assert_eq!(gpr[4], 0x43);
    }

    #[test]
    fn thumb_mem_block_requires_bus() {
        let mut compiler = DynarecCompiler::new();
        let ldr = 0x6941u16;
        assert!(compiler.try_compile_thumb_mem_block(&[ldr]).is_none());
    }

    // --- Thumb BX as block terminator ---

    #[test]
    fn decode_thumb_bx_shapes() {
        // BX R1  -> 010001_11_0_0_001_000 = 0b0100_0111_0000_1000 = 0x4708
        let d = DynarecCompiler::decode_thumb_bx(0x4708).expect("BX R1");
        assert_eq!(d.rs, 1);

        // BX R14 (LR)  -> 010001_11_0_1_110_000 = 0b0100_0111_0111_0000 = 0x4770
        let d = DynarecCompiler::decode_thumb_bx(0x4770).expect("BX LR");
        assert_eq!(d.rs, 14);

        // H1 set is SBZ violation -> reject.
        //   010001_11_1_0_001_000 = 0b0100_0111_1000_1000 = 0x4788
        assert!(DynarecCompiler::decode_thumb_bx(0x4788).is_none());

        // Low 3 bits nonzero is SBZ violation -> reject.
        assert!(DynarecCompiler::decode_thumb_bx(0x4709).is_none());

        // Not format 5 BX.
        assert!(DynarecCompiler::decode_thumb_bx(0x2000).is_none());
        assert!(DynarecCompiler::decode_thumb_bx(0x4488).is_none()); // ADD Hi
    }

    #[test]
    fn compile_thumb_bx_writes_target_and_returns_1() {
        let mut compiler = DynarecCompiler::new();
        // BX R1 (target in R1).
        let func = compiler
            .try_compile_thumb_block_with_branch(&[0x4708], 0x0800_0000)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[1] = 0x0800_1234; // target, bit 0 = 0 -> ARM mode
        let mut cpsr = 0u32;
        let mut pc_out = 0xDEAD_BEEFu32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 1);
        assert_eq!(pc_out, 0x0800_1234);
    }

    #[test]
    fn compile_thumb_bx_preserves_thumb_bit() {
        let mut compiler = DynarecCompiler::new();
        // BX R2
        //   010001_11_0_0_010_000 = 0x4710
        let func = compiler
            .try_compile_thumb_block_with_branch(&[0x4710], 0)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[2] = 0x0800_1235; // bit 0 = 1 -> Thumb mode
        let mut cpsr = 0u32;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 1);
        assert_eq!(pc_out & 1, 1, "Thumb bit preserved");
        assert_eq!(pc_out & !1, 0x0800_1234);
    }

    #[test]
    fn compile_thumb_block_with_body_and_bx_tail() {
        let mut compiler = DynarecCompiler::new();
        // MOV R0, #5 ; ADD R1, R0, #3 ; BX LR
        let mov = 0x2005u16;        // fmt 3
        let add_imm3 = 0x1CC1u16;   // ADD R1, R0, #3 -> 00011_10_011_000_001 = 0b0001_1100_1100_0001
        let bx_lr = 0x4770u16;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[mov, add_imm3, bx_lr], 0x0800_2000)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[14] = 0x0800_3001; // LR: bit 0 set -> Thumb
        let mut cpsr = 0u32;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(gpr[0], 5);
        assert_eq!(gpr[1], 8);
        assert_eq!(taken, 1);
        assert_eq!(pc_out, 0x0800_3001);
    }

    #[test]
    fn compile_thumb_block_no_bx_returns_0() {
        let mut compiler = DynarecCompiler::new();
        // Body only, no BX. Should return 0 and leave pc_out alone.
        let mov = 0x2042u16;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[mov], 0)
            .unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out = 0xC0FF_EE00u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 0);
        assert_eq!(pc_out, 0xC0FF_EE00);
        assert_eq!(gpr[0], 0x42);
    }

    #[test]
    fn decode_thumb_format18_unconditional() {
        // B #+8 (forward). imm11 offset from instr+4. imm11 = 0 means target
        // = instr+4+0 = instr+4 (skip no instrs).
        //   11100_00000000000 = 0xE000
        let d = DynarecCompiler::decode_thumb_format18(0xE000).expect("B +0");
        assert_eq!(d.cond, ArmCond::Al);
        assert_eq!(d.offset_signed, 0);

        // Forward 4 instructions: imm11 = 4 -> 0xE004
        let d = DynarecCompiler::decode_thumb_format18(0xE004).unwrap();
        assert_eq!(d.offset_signed, 4);

        // Backward: imm11 = -1 -> 0xE7FF
        let d = DynarecCompiler::decode_thumb_format18(0xE7FF).unwrap();
        assert_eq!(d.offset_signed, -1);

        // Not format 18.
        assert!(DynarecCompiler::decode_thumb_format18(0xE800).is_none());
        assert!(DynarecCompiler::decode_thumb_format18(0x2000).is_none());
    }

    #[test]
    fn decode_thumb_format16_conditional() {
        // BEQ #+4 (cond=0x0, imm8=2 -> target = pc+4+4)
        //   1101_0000_00000010 = 0xD002
        let d = DynarecCompiler::decode_thumb_format16(0xD002).expect("BEQ");
        assert_eq!(d.cond, ArmCond::Eq);
        assert_eq!(d.offset_signed, 2);

        // BNE -4: cond=0x1, imm8 = -2 -> 0xD1FE
        let d = DynarecCompiler::decode_thumb_format16(0xD1FE).unwrap();
        assert_eq!(d.cond, ArmCond::Ne);
        assert_eq!(d.offset_signed, -2);

        // cond AL (0xE) is reserved for format 18, reject here.
        assert!(DynarecCompiler::decode_thumb_format16(0xDE00).is_none());
        // cond 0xF is SWI (format 17), reject.
        assert!(DynarecCompiler::decode_thumb_format16(0xDF00).is_none());
        // Not format 16 at all.
        assert!(DynarecCompiler::decode_thumb_format16(0xC000).is_none());
    }

    #[test]
    fn compile_thumb_format18_taken() {
        let mut compiler = DynarecCompiler::new();
        // B +0: target = branch_pc + 4 + 0, with Thumb bit.
        let b = 0xE000u16;
        let entry_pc = 0x0800_1000u32;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[b], entry_pc)
            .unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 1);
        // branch_pc = entry_pc (body_len = 0). target = entry_pc + 4 | 1.
        assert_eq!(pc_out, (entry_pc + 4) | 1);
    }

    #[test]
    fn compile_thumb_format16_not_taken() {
        let mut compiler = DynarecCompiler::new();
        // BEQ #+4 with Z=0.
        let beq = 0xD002u16;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[beq], 0x0800_2000)
            .unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32; // Z=0
        let mut pc_out = 0xBADD_F00Du32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 0);
        assert_eq!(pc_out, 0xBADD_F00Du32);
    }

    #[test]
    fn compile_thumb_format16_taken_when_z_set() {
        let mut compiler = DynarecCompiler::new();
        let beq = 0xD002u16;
        let entry_pc = 0x0800_3000u32;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[beq], entry_pc)
            .unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr: u32 = 1 << 30;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(taken, 1);
        assert_eq!(pc_out, (entry_pc + 4 + 4) | 1);
    }

    #[test]
    fn compile_thumb_body_then_b_tail() {
        let mut compiler = DynarecCompiler::new();
        // MOV R0, #1 ; B -0
        //   MOV R0, #1 -> 0x2001
        //   B -0 means backward to self (imm11 = -2 -> 0xE7FE)
        let mov = 0x2001u16;
        let b   = 0xE7FEu16;
        let entry_pc = 0x0800_4000u32;
        let func = compiler
            .try_compile_thumb_block_with_branch(&[mov, b], entry_pc)
            .unwrap();

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out = 0u32;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);
        assert_eq!(gpr[0], 1);
        assert_eq!(taken, 1);
        // branch_pc = entry_pc + 2 (one body instr). target = branch_pc + 4 + (-2 << 1) = branch_pc.
        // That's entry_pc + 2. With Thumb bit = (entry_pc + 2) | 1.
        assert_eq!(pc_out, (entry_pc + 2) | 1);
    }

    #[test]
    fn compile_thumb_mid_block_pc_branch_rejected() {
        let mut compiler = DynarecCompiler::new();
        let b  = 0xE000u16; // format 18
        let mov = 0x2042u16;
        assert!(compiler
            .try_compile_thumb_block_with_branch(&[b, mov], 0)
            .is_none());
    }

    #[test]
    fn compile_thumb_mid_block_bx_rejected() {
        let mut compiler = DynarecCompiler::new();
        // BX can only be in the last slot.
        let bx = 0x4708u16;
        let mov = 0x2005u16;
        assert!(compiler
            .try_compile_thumb_block_with_branch(&[bx, mov], 0)
            .is_none());
    }

    // --- Thumb format 5 (ADD/CMP/MOV Hi) ---

    #[test]
    fn decode_thumb_format5_shapes() {
        // ADD R8, R0, R1  (H1=1, H2=0, Rd=0, Rs=1)
        //   010001_00_10_001_000 = 0b0100_0100_1000_1000 = 0x4488
        let d = DynarecCompiler::decode_thumb_format5_non_branch(0x4488).expect("ADD Hi");
        assert_eq!(d.op, Thumb5Op::Add);
        assert_eq!(d.rd, 8);
        assert_eq!(d.rs, 1);

        // MOV R9, R0  (H1=1, H2=0, op=10)
        //   010001_10_10_000_001 = 0b0100_0110_1000_0001 = 0x4681
        let d = DynarecCompiler::decode_thumb_format5_non_branch(0x4681).expect("MOV Hi");
        assert_eq!(d.op, Thumb5Op::Mov);
        assert_eq!(d.rd, 9);
        assert_eq!(d.rs, 0);

        // CMP R10, R11  (H1=1, H2=1, op=01)
        //   010001_01_11_011_010 = 0b0100_0101_1101_1010 = 0x45DA
        let d = DynarecCompiler::decode_thumb_format5_non_branch(0x45DA).expect("CMP Hi");
        assert_eq!(d.op, Thumb5Op::Cmp);
        assert_eq!(d.rd, 10);
        assert_eq!(d.rs, 11);

        // BX encoding (oo=11) must reject here.
        //   010001_11_00_001_000 = 0b0100_0111_0000_1000 = 0x4708  BX R1
        assert!(DynarecCompiler::decode_thumb_format5_non_branch(0x4708).is_none());

        // H1=0, H2=0, op=ADD is UNPREDICTABLE -> reject.
        //   010001_00_00_001_000 = 0b0100_0100_0000_1000 = 0x4408
        assert!(DynarecCompiler::decode_thumb_format5_non_branch(0x4408).is_none());

        // PC (R15) source or dest -> reject.
        //   ADD R15, R0  010001_00_10_000_111 = 0x4487  (Rd=7, H1=1 -> 15)
        assert!(DynarecCompiler::decode_thumb_format5_non_branch(0x4487).is_none());
    }

    #[test]
    fn compile_thumb_format5_add_mov_no_flag_update() {
        let mut compiler = DynarecCompiler::new();
        // ADD R8, R1  (H1=1)  -> R8 += R1
        //   010001_00_10_001_000 = 0x4488
        // MOV R9, R2  (H1=1)
        //   010001_10_10_010_001 = 0b0100_0110_1001_0001 = 0x4691
        let add = 0x4488u16;
        let mov = 0x4691u16;
        let func = compiler
            .try_compile_thumb_block(&[add, mov])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[1] = 5;
        gpr[2] = 42;
        gpr[8] = 10;
        let mut cpsr: u32 = (1 << 30) | (1 << 31); // Z=1, N=1 pre-set
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[8], 15);
        assert_eq!(gpr[9], 42);
        // ADD/MOV in format 5 don't set flags -> N and Z stay where they were.
        assert_ne!(cpsr & (1 << 30), 0, "Z preserved");
        assert_ne!(cpsr & (1 << 31), 0, "N preserved");
    }

    #[test]
    fn compile_thumb_format5_cmp_sets_flags_no_writeback() {
        let mut compiler = DynarecCompiler::new();
        // CMP R10, R11  -> 0x45DA
        let cmp = 0x45DAu16;
        let func = compiler.try_compile_thumb_block(&[cmp]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[10] = 7;
        gpr[11] = 7;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[10], 7);
        assert_eq!(gpr[11], 7);
        assert_ne!(cpsr & (1 << 30), 0, "Z set on equal");
    }

    // --- Thumb format 1 (LSL/LSR/ASR Rd, Rs, #imm5) ---

    #[test]
    fn decode_thumb_format1_shapes() {
        // LSL R0, R1, #3  -> 000_00_00011_001_000 = 0b0000_0000_1100_1000 = 0x00C8
        let d = DynarecCompiler::decode_thumb_format1(0x00C8).expect("LSL");
        assert_eq!(d.kind, ShiftKind::Lsl);
        assert_eq!(d.imm5, 3);
        assert_eq!(d.rs, 1);
        assert_eq!(d.rd, 0);

        // LSR R2, R3, #5  -> 000_01_00101_011_010 = 0b0000_1001_0101_1010 = 0x095A
        let d = DynarecCompiler::decode_thumb_format1(0x095A).expect("LSR");
        assert_eq!(d.kind, ShiftKind::Lsr);
        assert_eq!(d.imm5, 5);

        // ASR R5, R6, #8  -> 000_10_01000_110_101 = 0b0001_0010_0011_0101 = 0x1235
        let d = DynarecCompiler::decode_thumb_format1(0x1235).expect("ASR");
        assert_eq!(d.kind, ShiftKind::Asr);
        assert_eq!(d.imm5, 8);

        // oo = 11 (format 2) must be rejected by format 1 decoder.
        assert!(DynarecCompiler::decode_thumb_format1(0x1800).is_none());

        // Not Thumb format 1 (top 3 bits != 000).
        assert!(DynarecCompiler::decode_thumb_format1(0x2000).is_none()); // fmt 3
        assert!(DynarecCompiler::decode_thumb_format1(0x4000).is_none()); // fmt 4
    }

    #[test]
    fn compile_lsl_imm_regular() {
        let mut compiler = DynarecCompiler::new();
        // LSL R0, R1, #3
        let func = compiler
            .try_compile_thumb_block(&[0x00C8])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[1] = 0x0000_0005;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 5 << 3);
        // C = bit (32-3) = bit 29 of 5 = 0.
        assert_eq!(cpsr & (1 << 29), 0);
    }

    #[test]
    fn compile_lsl_zero_preserves_c() {
        let mut compiler = DynarecCompiler::new();
        // LSL R0, R1, #0  -> no shift, C preserved.
        let func = compiler
            .try_compile_thumb_block(&[0x0008])
            .unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 0xDEAD_BEEF;
        let mut cpsr: u32 = 1 << 29; // Set C = 1 on input.
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0xDEAD_BEEF);
        assert_ne!(cpsr & (1 << 29), 0, "C preserved on LSL #0");
    }

    #[test]
    fn compile_lsl_imm_shifts_out_carry() {
        let mut compiler = DynarecCompiler::new();
        // LSL R0, R1, #1 -> R0 = R1 << 1, C = bit 31 of R1.
        let func = compiler
            .try_compile_thumb_block(&[0x0048])
            .unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 0x8000_0001;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0x0000_0002);
        assert_ne!(cpsr & (1 << 29), 0, "C from shifted out top bit");
    }

    #[test]
    fn compile_lsr_zero_is_lsr_32() {
        let mut compiler = DynarecCompiler::new();
        // LSR R0, R1, #0 -> LSR #32: result = 0, C = bit 31 of R1.
        let func = compiler
            .try_compile_thumb_block(&[0x0808])
            .unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 0x8000_0000;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0);
        assert_ne!(cpsr & (1 << 29), 0, "C = bit 31 of Rs");
        assert_ne!(cpsr & (1 << 30), 0, "Z set when result == 0");
    }

    #[test]
    fn compile_asr_preserves_sign() {
        let mut compiler = DynarecCompiler::new();
        // ASR R0, R1, #4 -> arithmetic right shift.
        let func = compiler
            .try_compile_thumb_block(&[0x1108])
            .unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 0x8000_0000u32;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0xF800_0000u32, "ASR sign extends");
        assert_ne!(cpsr & (1 << 31), 0, "N set (result top bit)");
    }

    #[test]
    fn compile_asr_zero_is_asr_32() {
        let mut compiler = DynarecCompiler::new();
        // ASR R0, R1, #0 -> ASR #32: result = all sign bits of Rs.
        let func = compiler
            .try_compile_thumb_block(&[0x1008])
            .unwrap();

        let mut gpr = [0u32; 15];
        gpr[1] = 0x8000_0000u32;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0xFFFF_FFFFu32, "all ones when sign bit set");

        gpr[1] = 0x0000_0001u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0, "all zeros when sign bit clear");
    }

    // --- Thumb format 4 logical subset ---

    #[test]
    fn decode_thumb_format4_logical_shapes() {
        // AND R0, R1 -> 010000_0000_001_000 = 0b0100_0000_0000_1000 = 0x4008
        let d = DynarecCompiler::decode_thumb_format4_logical(0x4008).expect("AND");
        assert_eq!(d.op, Thumb4Op::And);
        assert_eq!(d.rd, 0);
        assert_eq!(d.rs, 1);

        // ORR R2, R3 -> 010000_1100_011_010 = 0x431A
        let d = DynarecCompiler::decode_thumb_format4_logical(0x431A).expect("ORR");
        assert_eq!(d.op, Thumb4Op::Orr);
        assert_eq!(d.rs, 3);
        assert_eq!(d.rd, 2);

        // TST R4, R5 -> 010000_1000_101_100 = 0x422C
        let d = DynarecCompiler::decode_thumb_format4_logical(0x422C).expect("TST");
        assert_eq!(d.op, Thumb4Op::Tst);

        // CMP R6, R7 -> 010000_1010_111_110 = 0x42BE
        let d = DynarecCompiler::decode_thumb_format4_logical(0x42BE).expect("CMP");
        assert_eq!(d.op, Thumb4Op::Cmp);

        // MVN R1, R2 -> 010000_1111_010_001 = 0x43D1
        let d = DynarecCompiler::decode_thumb_format4_logical(0x43D1).expect("MVN");
        assert_eq!(d.op, Thumb4Op::Mvn);

        // LSL R0, R1 -> 010000_0010_001_000 = 0x4088  (unsupported)
        assert!(DynarecCompiler::decode_thumb_format4_logical(0x4088).is_none());
        // MUL R0, R1 -> 010000_1101_001_000 = 0x4348  (unsupported)
        assert!(DynarecCompiler::decode_thumb_format4_logical(0x4348).is_none());

        // Not format 4 (top 6 bits != 010000)
        assert!(DynarecCompiler::decode_thumb_format4_logical(0x2000).is_none());
    }

    #[test]
    fn compile_thumb_and_orr_eor_bic() {
        let mut compiler = DynarecCompiler::new();
        // AND R0, R1 ; ORR R2, R3 ; EOR R4, R5 ; BIC R6, R7
        let and = 0x4008u16; // 010000_0000_001_000
        let orr = 0x431Au16; // 010000_1100_011_010
        let eor = 0x406Cu16; // 010000_0001_101_100
        let bic = 0x43BEu16; // 010000_1110_111_110
        let func = compiler
            .try_compile_thumb_block(&[and, orr, eor, bic])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0xF0F0_F0F0; gpr[1] = 0x0FF0_0FF0;
        gpr[2] = 0x0000_0001; gpr[3] = 0x0000_0010;
        gpr[4] = 0xAAAA_AAAA; gpr[5] = 0x5555_5555;
        gpr[6] = 0xFFFF_FFFF; gpr[7] = 0x0F0F_0F0F;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 0xF0F0_F0F0 & 0x0FF0_0FF0, "AND R0, R1");
        assert_eq!(gpr[2], 0x0000_0001 | 0x0000_0010, "ORR R2, R3");
        assert_eq!(gpr[4], 0xAAAA_AAAA ^ 0x5555_5555, "EOR R4, R5");
        assert_eq!(gpr[6], 0xFFFF_FFFF & !0x0F0F_0F0F, "BIC R6, R7");
    }

    #[test]
    fn compile_thumb_mvn_complements() {
        let mut compiler = DynarecCompiler::new();
        // MVN R1, R2
        let mvn = 0x43D1u16;
        let func = compiler.try_compile_thumb_block(&[mvn]).unwrap();

        let mut gpr = [0u32; 15];
        gpr[2] = 0x1234_5678;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[1], !0x1234_5678u32);
        // Top bit set -> N should be set.
        assert_ne!(cpsr & (1 << 31), 0, "N set on negative result");
    }

    #[test]
    fn compile_thumb_tst_cmp_cmn_no_writeback() {
        let mut compiler = DynarecCompiler::new();
        // TST R4, R5 ; CMP R4, R5 ; CMN R4, R5
        let tst = 0x422Cu16;
        let cmp = 0x42ACu16; // 010000_1010_101_100 = 0x42AC
        let cmn = 0x42ECu16; // 010000_1011_101_100 = 0x42EC
        let func = compiler
            .try_compile_thumb_block(&[tst, cmp, cmn])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[4] = 0xDEAD_BEEF;
        gpr[5] = 0x1111_1111;
        let before = gpr;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr, before, "TST/CMP/CMN never writeback");
    }

    #[test]
    fn compile_thumb_mix_format2_and_3() {
        let mut compiler = DynarecCompiler::new();
        // MOV R1, #5; MOV R2, #3; ADD R0, R1, R2; SUB R0, R0, #1
        let mov_r1_5  = 0x2105u16;                    // fmt 3
        let mov_r2_3  = 0x2203u16;                    // fmt 3
        let add_r0_r1_r2 = 0x1888u16;                 // fmt 2 reg
        let sub_r0_r0_1  = 0x1E40u16;                 // fmt 2 imm3: 00011_11_001_000_000 = 0x1E40
        let func = compiler
            .try_compile_thumb_block(&[mov_r1_5, mov_r2_3, add_r0_r1_r2, sub_r0_r0_1])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr);
        assert_eq!(gpr[0], 7);
        assert_eq!(gpr[1], 5);
        assert_eq!(gpr[2], 3);
    }

    #[test]
    fn compile_ldr_negative_offset() {
        let (bus, mut compiler) = new_bus_and_compiler();
        unsafe {
            let b = &mut *bus.bytes.get();
            b[0x60] = 0x01; b[0x61] = 0x02; b[0x62] = 0x03; b[0x63] = 0x04;
        }
        // LDR R1, [R0, #-4]  R0 starts at 0x64, addr = 0x60
        //   E5_10_10_04
        let ldr_neg = 0xE510_1004u32;
        let func = compiler
            .try_compile_mem_block(&[ldr_neg])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0x64;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0x0403_0201u32);
    }

    #[test]
    fn compile_ldr_single() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // Seed memory at offset 0x20.
        unsafe {
            let b = &mut *bus.bytes.get();
            b[0x20] = 0xAA; b[0x21] = 0xBB; b[0x22] = 0xCC; b[0x23] = 0xDD;
        }

        // LDR R1, [R0, #0x20]
        let ldr = 0xE590_1020u32;
        let func = compiler
            .try_compile_mem_block(&[ldr])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0; // base = 0
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0xDDCC_BBAA);
    }

    #[test]
    fn compile_str_then_ldr_roundtrip() {
        let (bus, mut compiler) = new_bus_and_compiler();

        // MOV R2, #0x55 (... actually use the imm that encodes simply)
        // We'll set R2 and R3 via gpr initial state and just emit the mem ops.
        // STR R2, [R0, #0x10];  LDR R3, [R0, #0x10]
        let str_ = 0xE580_2010u32;
        let ldr = 0xE590_3010u32;
        let func = compiler
            .try_compile_mem_block(&[str_, ldr])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        gpr[2] = 0x1234_5678;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[3], 0x1234_5678, "STR then LDR should round trip");
        // And the byte layout in the buffer should be little endian.
        unsafe {
            let b = &*bus.bytes.get();
            assert_eq!(b[0x10], 0x78);
            assert_eq!(b[0x11], 0x56);
            assert_eq!(b[0x12], 0x34);
            assert_eq!(b[0x13], 0x12);
        }
    }

    #[test]
    fn compile_mixed_dp_and_mem() {
        let (bus, mut compiler) = new_bus_and_compiler();
        // MOV R2, #0x42;  STR R2, [R0, #4];  LDR R3, [R0, #4];  ADD R4, R3, #1
        let mov  = 0xE3A0_2042u32;
        let str_ = 0xE580_2004u32;
        let ldr  = 0xE590_3004u32;
        let add  = 0xE283_4001u32;
        let func = compiler
            .try_compile_mem_block(&[mov, str_, ldr, add])
            .expect("compiles");

        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[2], 0x42);
        assert_eq!(gpr[3], 0x42);
        assert_eq!(gpr[4], 0x43);
    }

    #[test]
    fn compile_mem_requires_bus() {
        let mut compiler = DynarecCompiler::new();  // no bus
        let ldr = 0xE590_1020u32;
        assert!(compiler.try_compile_mem_block(&[ldr]).is_none());
    }

    #[test]
    fn compile_ldr_conditional_not_taken() {
        let (bus, mut compiler) = new_bus_and_compiler();
        unsafe {
            let b = &mut *bus.bytes.get();
            b[0] = 0x11; b[1] = 0x22; b[2] = 0x33; b[3] = 0x44;
        }
        // LDREQ R1, [R0, #0]
        //   cond=EQ (0x0), rest same as LDR pattern above.
        //   0_590_1000 = 0x0590_1000
        let ldreq = 0x0590_1000u32;
        let func = compiler
            .try_compile_mem_block(&[ldreq])
            .expect("compiles");

        // Z=0 -> not taken -> gpr[1] stays 0
        let mut gpr = [0u32; 15];
        gpr[1] = 0xDEAD_BEEF;
        let mut cpsr = 0u32;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0xDEAD_BEEF, "EQ not taken, no load");

        // Z=1 -> taken
        let mut gpr = [0u32; 15];
        gpr[0] = 0;
        let mut cpsr: u32 = 1 << 30;
        func(gpr.as_mut_ptr(), &mut cpsr, &bus as *const TestBus as *mut u8);
        assert_eq!(gpr[1], 0x4433_2211);
    }

    #[test]
    fn branch_block_with_dp_tail_also_works() {
        // When the block has no terminal B/BL, the compiled fn should just
        // run the DP instructions and return 0 (no branch taken).
        let mut compiler = DynarecCompiler::new();
        let mov0 = 0xE3A0_0005u32;
        let mov1 = 0xE3A0_100Au32;
        let func = compiler
            .try_compile_block_with_branch(&[mov0, mov1], 0x800_5000)
            .expect("compiles");

        let mut gpr = [0u32; 15];
        let mut cpsr = 0u32;
        let mut pc_out: u32 = 0xC0FF_EE00;
        let taken = func(gpr.as_mut_ptr(), &mut cpsr, &mut pc_out);

        assert_eq!(taken, 0);
        assert_eq!(pc_out, 0xC0FF_EE00, "no branch, pc_out untouched");
        assert_eq!(gpr[0], 5);
        assert_eq!(gpr[1], 10);
    }
}
