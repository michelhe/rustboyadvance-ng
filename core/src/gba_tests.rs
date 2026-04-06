//! Integration tests for jsmolka/gba-tests ROMs.
//!
//! Each ROM renders "All tests passed" or "Failed test NNN" to the screen
//! (background mode 4) and stores a three-digit breakdown in IWRAM on failure:
//!   0x0300_0000 – hundreds digit
//!   0x0300_0004 – tens digit
//!   0x0300_0008 – ones digit
//!
//! The ROM falls into an infinite `b idle` branch after finishing so we can
//! detect completion by checking for the idle-loop opcode at the PC.
//!
//! When a test fails, the ROM uses `swi 0x60000` (BIOS DivMod, function #6) to
//! decompose the failing test number into its three decimal digits.  A minimal
//! ARM BIOS ROM is synthesised at runtime to handle that SWI so that the SWI
//! handler does not corrupt the CPU state.

#![cfg(test)]

use crate::prelude::*;
use arm7tdmi::{CpuState, memory::BusIO};

// ── minimal BIOS ROM ─────────────────────────────────────────────────────────

/// Build a minimal 16 KiB ARM BIOS ROM that handles SWI function 0x06
/// (GBA BIOS DivMod: r0 = r0/r1 [quotient], r1 = r0%r1 [remainder],
/// r3 = abs(quotient) = quotient since both operands are always positive in
/// our tests).  All other SWIs and exception vectors are handled as graceful
/// no-ops or infinite loops.
///
/// Memory map of the generated ROM:
/// ```text
/// 0x0000  B  halt          ; reset  → halt loop
/// 0x0004  B  halt          ; undef  → halt loop
/// 0x0008  B  swi_handler   ; SWI vector
/// 0x000C  B  halt          ; prefetch abort → halt loop
/// 0x0010  B  halt          ; data abort     → halt loop
/// 0x0014  MOV R0,R0        ; reserved (NOP)
/// 0x0018  SUBS PC,LR,#4    ; IRQ → return
/// 0x001C  SUBS PC,LR,#4    ; FIQ → return
///
/// 0x0020  LDR   R12,[LR,#-4]    ; read the SWI instruction
/// 0x0024  MOV   R12,R12,LSR #16 ; isolate bits 23:16 (function number)
/// 0x0028  AND   R12,R12,#0xFF
/// 0x002C  CMP   R12,#6          ; DivMod?
/// 0x0030  BNE   swi_return       ; no – just return
/// 0x0034  MOV   R12,#0           ; quotient = 0
/// 0x0038  CMP   R0,R1            ; loop: dividend >= divisor?
/// 0x003C  SUBHS R0,R0,R1         ;   subtract
/// 0x0040  ADDHS R12,R12,#1       ;   increment quotient
/// 0x0044  BHS   0x0038            ;   repeat
/// 0x0048  MOV   R1,R0            ; R1 = remainder
/// 0x004C  MOV   R0,R12           ; R0 = quotient
/// 0x0050  MOV   R3,R12           ; R3 = abs(quotient)
/// 0x0054  MOVS  PC,LR            ; return from SWI (swi_return)
///
/// 0x0058  B  0x0058              ; halt loop
/// ```
fn make_minimal_bios() -> Box<[u8]> {
    const BIOS_SIZE: usize = 0x4000;
    let mut bios = vec![0u8; BIOS_SIZE];

    // Helper: encode an unconditional ARM branch `B target` at `from`.
    // PC when executing = from + 8 (ARM pipeline).
    let b = |from: usize, to: usize| -> [u8; 4] {
        let offset = ((to as i32) - (from as i32) - 8) / 4;
        (0xEA00_0000u32 | (offset as u32 & 0x00FF_FFFF)).to_le_bytes()
    };

    // Exception vector table (0x0000 – 0x001F)
    bios[0x0000..0x0004].copy_from_slice(&b(0x0000, 0x0058)); // reset  → halt
    bios[0x0004..0x0008].copy_from_slice(&b(0x0004, 0x0058)); // undef  → halt
    bios[0x0008..0x000C].copy_from_slice(&b(0x0008, 0x0020)); // SWI    → handler
    bios[0x000C..0x0010].copy_from_slice(&b(0x000C, 0x0058)); // pabt   → halt
    bios[0x0010..0x0014].copy_from_slice(&b(0x0010, 0x0058)); // dabt   → halt
    bios[0x0014..0x0018].copy_from_slice(&0xE1A0_0000u32.to_le_bytes()); // NOP
    bios[0x0018..0x001C].copy_from_slice(&0xE25E_F004u32.to_le_bytes()); // IRQ: SUBS PC,LR,#4
    bios[0x001C..0x0020].copy_from_slice(&0xE25E_F004u32.to_le_bytes()); // FIQ: SUBS PC,LR,#4

    // SWI handler body (0x0020 – 0x0057)
    //
    // All instructions are hard-coded ARM machine words (little-endian).
    // Encoding was derived from the ARM Architecture Reference Manual; see
    // the module-level doc-comment for the instruction sequence.
    let swi_body: &[u32] = &[
        0xE51E_C004, // LDR   R12,[LR,#-4]
        0xE1A0_C82C, // MOV   R12,R12,LSR #16
        0xE20C_C0FF, // AND   R12,R12,#0xFF
        0xE35C_0006, // CMP   R12,#6
        0x1A00_0007, // BNE   swi_return  (offset +7 words → 0x0054)
        0xE3A0_C000, // MOV   R12,#0      (quotient = 0)
        0xE150_0001, // CMP   R0,R1       (loop start @ 0x0038)
        0x2040_0001, // SUBHS R0,R0,R1
        0x228C_C001, // ADDHS R12,R12,#1
        0x2AFF_FFFB, // BHS   0x0038      (offset -5 words)
        0xE1A0_1000, // MOV   R1,R0       (remainder)
        0xE1A0_000C, // MOV   R0,R12      (quotient)
        0xE1A0_300C, // MOV   R3,R12      (abs quotient)
        0xE1B0_F00E, // MOVS  PC,LR       (swi_return: restore CPSR, return)
    ];
    for (i, &word) in swi_body.iter().enumerate() {
        let off = 0x0020 + i * 4;
        bios[off..off + 4].copy_from_slice(&word.to_le_bytes());
    }

    // Halt loop (0x0058)
    bios[0x0058..0x005C].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B 0x0058

    bios.into_boxed_slice()
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_mock_gba(rom: &[u8]) -> GameBoyAdvance {
    let bios = make_minimal_bios();
    let cartridge = GamepakBuilder::new()
        .buffer(rom)
        .with_sram()
        .without_backup_to_file()
        .build()
        .unwrap();
    let mut gba = GameBoyAdvance::new(bios, cartridge, NullAudio::new());
    gba.skip_bios();
    gba
}

/// Outcome of a single gba-tests ROM run.
enum GbaTestResult {
    Passed,
    /// The first failing test number (1-based, as printed on screen).
    Failed(u32),
    /// The ROM did not reach the idle loop within the frame budget.
    /// The `pc` field contains the last observed program counter.
    DidNotTerminate { pc: u32 },
}

/// Run `rom` for up to `max_frames` and return whether it passed.
///
/// Completion is detected by the idle-loop opcode at the PC:
/// - ARM:   `0xEAFFFFFE`  (`b idle` — branches back to itself, offset -2)
/// - THUMB: `0xE7FE`      (`b idle`)
///
/// The failing test number (0 when all tests pass) is read from `result_reg`
/// — the CPU register the ROM stores its running test-failure count in.  ARM
/// mode test ROMs use R12; the THUMB test ROM uses R7.
fn run_gba_test(rom: &[u8], result_reg: usize, max_frames: usize) -> GbaTestResult {
    const ARM_IDLE: u32 = 0xEAFF_FFFE;
    const THUMB_IDLE: u16 = 0xE7FE;

    let mut gba = make_mock_gba(rom);

    for _ in 0..max_frames {
        gba.frame();

        // Detect idle loop (ARM or THUMB state).
        let pc = gba.cpu.pc;
        let in_thumb = gba.cpu.cpsr.state() == CpuState::THUMB;

        let reached_idle = if in_thumb {
            gba.sysbus.read_16(pc.wrapping_sub(4)) == THUMB_IDLE
        } else {
            gba.sysbus.read_32(pc.wrapping_sub(8)) == ARM_IDLE
        };

        if reached_idle {
            // The ROM stores the first failing test number (or 0) in result_reg.
            let test_num = gba.cpu.gpr[result_reg];
            return if test_num == 0 {
                GbaTestResult::Passed
            } else {
                GbaTestResult::Failed(test_num)
            };
        }
    }

    GbaTestResult::DidNotTerminate { pc: gba.cpu.pc }
}

// ── section-name helpers ──────────────────────────────────────────────────────

fn arm_section(n: u32) -> &'static str {
    match n {
        1..=49 => "conditions",
        50..=99 => "branches",
        100..=149 => "flags",
        150..=199 => "shifts",
        200..=249 => "data_processing",
        250..=299 => "psr_transfer",
        300..=349 => "multiply",
        350..=399 => "single_transfer",
        400..=449 => "halfword_transfer",
        450..=499 => "data_swap",
        500..=u32::MAX => "block_transfer",
        0 => "unknown",
    }
}

fn thumb_section(n: u32) -> &'static str {
    match n {
        1..=49 => "logical",
        50..=99 => "shifts",
        100..=149 => "arithmetic",
        150..=199 => "branches",
        200..=u32::MAX => "memory",
        0 => "unknown",
    }
}

fn memory_section(n: u32) -> &'static str {
    match n {
        1..=49 => "mirrors",
        50..=u32::MAX => "video_strb",
        0 => "unknown",
    }
}

// ── test cases ────────────────────────────────────────────────────────────────

#[test]
fn test_gba_tests_arm() {
    match run_gba_test(include_bytes!("../../external/gba-tests/arm/arm.gba"), 12, 300) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!(
            "arm.gba failed at test {} (section: {})",
            n,
            arm_section(n)
        ),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("arm.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}

#[test]
fn test_gba_tests_thumb() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/thumb/thumb.gba"),
        7,
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!(
            "thumb.gba failed at test {} (section: {})",
            n,
            thumb_section(n)
        ),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("thumb.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}

/// bios.gba tests the content of the GBA BIOS ROM at specific addresses and the
/// behaviour of BIOS SWI functions (sqrt, wait-for-vblank, IRQ handler stubs …).
/// All of these require a genuine GBA BIOS ROM, which is proprietary and cannot
/// be included in this repository.  The minimal synthetic BIOS used for all
/// other tests is intentionally incomplete, so this test is expected to fail
/// and is therefore skipped by default.
#[ignore = "requires a real GBA BIOS ROM"]
#[test]
fn test_gba_tests_bios() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/bios/bios.gba"),
        12,
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!("bios.gba failed at test {}", n),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("bios.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}

#[test]
fn test_gba_tests_memory() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/memory/memory.gba"),
        12,
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!(
            "memory.gba failed at test {} (section: {})",
            n,
            memory_section(n)
        ),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("memory.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}

#[test]
fn test_gba_tests_unsafe() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/unsafe/unsafe.gba"),
        12,
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!("unsafe.gba failed at test {}", n),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("unsafe.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}

#[test]
fn test_gba_tests_nes() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/nes/nes.gba"),
        12,
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!("nes.gba failed at test {}", n),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("nes.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}
