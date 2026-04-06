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

#![cfg(test)]

use crate::prelude::*;
use arm7tdmi::{CpuState, memory::BusIO};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_mock_gba(rom: &[u8]) -> GameBoyAdvance {
    let bios = vec![0; 0x4000].into_boxed_slice();
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
/// Completion is detected by the idle-loop opcode sitting at the PC:
/// - ARM:   `0xEAFFFFFE`  (`b idle` — branches back to itself, offset -2)
/// - THUMB: `0xE7FE`      (`b idle`)
///
/// On failure the ROM stores the three decimal digits of the failed test
/// number in IWRAM starting at `MEM_IWRAM` (0x0300_0000).
fn run_gba_test(rom: &[u8], max_frames: usize) -> GbaTestResult {
    const MEM_IWRAM: u32 = 0x0300_0000;
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
            let hundreds = gba.sysbus.read_32(MEM_IWRAM);
            let tens = gba.sysbus.read_32(MEM_IWRAM + 4);
            let ones = gba.sysbus.read_32(MEM_IWRAM + 8);

            return if hundreds == 0 && tens == 0 && ones == 0 {
                GbaTestResult::Passed
            } else {
                GbaTestResult::Failed(hundreds * 100 + tens * 10 + ones)
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
        _ => "block_transfer",
    }
}

fn thumb_section(n: u32) -> &'static str {
    match n {
        1..=49 => "logical",
        50..=99 => "shifts",
        100..=149 => "arithmetic",
        150..=199 => "branches",
        _ => "memory",
    }
}

fn memory_section(n: u32) -> &'static str {
    match n {
        1..=49 => "mirrors",
        _ => "video_strb",
    }
}

// ── test cases ────────────────────────────────────────────────────────────────

#[test]
fn test_gba_tests_arm() {
    match run_gba_test(include_bytes!("../../external/gba-tests/arm/arm.gba"), 300) {
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

#[test]
fn test_gba_tests_bios() {
    match run_gba_test(
        include_bytes!("../../external/gba-tests/bios/bios.gba"),
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
        300,
    ) {
        GbaTestResult::Passed => {}
        GbaTestResult::Failed(n) => panic!("nes.gba failed at test {}", n),
        GbaTestResult::DidNotTerminate { pc } => {
            panic!("nes.gba did not reach idle loop (last PC: {:#010x})", pc)
        }
    }
}
