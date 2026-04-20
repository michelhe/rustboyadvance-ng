//! Block cache for the cached interpreter.
//!
//! The baseline interpreter in `cpu.rs` fetches every instruction, hashes it
//! into the 4096-entry ARM LUT or the 1024-entry Thumb LUT, calls the
//! resolved handler function, and returns to the `single_step()` wrapper —
//! which then re-checks bus-master state and IRQ pending. A lot of that is
//! redundant across a run of non-branching instructions.
//!
//! This module stores, per (entry-PC, CPU state) pair, the sequence of raw
//! instruction words and their pre-resolved handler fn pointers that were
//! executed until the pipeline flushed. Re-entry at the same PC executes the
//! recorded sequence with a tight loop that skips the LUT hash and the
//! per-instruction single_step wrapper overhead.
//!
//! Accuracy is preserved because:
//!   * Memory-access cycle costs are paid by the handlers' `load_*`/`store_*`
//!     calls — those still run exactly as before, advancing the scheduler.
//!   * Scheduler events do not fire mid-instruction today (they fire between
//!     `single_step()` calls in `gba.rs::run`), and this module preserves
//!     that by returning to the outer loop after each executed block — which
//!     is at least as often as a pipeline flush.
//!   * Self-modifying code is handled by flushing the cache on any write to
//!     writable memory regions (see `SysBus::write_*`).

use std::collections::HashMap;
use std::rc::Rc;

use crate::cpu::{Arm7tdmiCore, CpuAction};
use crate::memory::MemoryInterface;

/// One recorded instruction inside a block.
///
/// Handler fn signatures differ between ARM and Thumb (u32 vs u16 insn word),
/// so we encode the mode in an enum. A uniform `u32`-argument handler would
/// require trampolines; this enum costs one jump-table branch per instruction
/// but keeps the handlers untouched.
#[derive(Clone, Copy)]
pub enum DecodedInstr<I: MemoryInterface> {
    Arm {
        raw: u32,
        handler: fn(&mut Arm7tdmiCore<I>, u32) -> CpuAction,
    },
    Thumb {
        raw: u16,
        handler: fn(&mut Arm7tdmiCore<I>, u16) -> CpuAction,
    },
}

/// Cache key = entry PC with the Thumb bit folded into bit 0.
/// ARM entry PCs are 4-byte aligned so bit 0 is always 0 in ARM mode;
/// Thumb entry PCs are 2-byte aligned so bit 0 is free for us.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockKey(u32);

impl BlockKey {
    #[inline]
    pub fn new(pc: u32, thumb: bool) -> Self {
        BlockKey(pc | (thumb as u32))
    }
}

/// A recorded straight-line run of instructions, terminated by whatever
/// PipelineFlushed the last time we executed it.
pub struct Block<I: MemoryInterface> {
    pub instrs: Vec<DecodedInstr<I>>,
}

impl<I: MemoryInterface> Block<I> {
    fn new() -> Self {
        Block { instrs: Vec::with_capacity(8) }
    }
}

/// Per-CPU block cache.
///
/// Blocks are held behind `Rc` so the executor can clone a handle at block
/// entry and then safely call handlers that may mutate memory (and therefore
/// invalidate the cache) without dangling references.
///
/// The cache is split into two maps keyed by entry-PC region:
///   * `rom_blocks` — blocks whose first instruction lives in BIOS or ROM.
///     These addresses are read-only on a real GBA (and by this emulator's
///     SysBus, which treats writes to BIOS/ROM as no-ops), so no write can
///     invalidate them. Keeping them warm across RAM writes is the main win
///     on games like pokeemerald that hammer IWRAM for state updates.
///   * `ram_blocks` — blocks starting in EWRAM or IWRAM. Flushed whenever
///     any RAM write happens, since self-modifying code (rare but legal)
///     could have overwritten a cached instruction.
///
/// Empty in the default build — the struct and all its methods compile to
/// no-ops unless the `cached_interp` feature is on.
pub struct BlockCache<I: MemoryInterface> {
    rom_blocks: HashMap<BlockKey, Rc<Block<I>>>,
    ram_blocks: HashMap<BlockKey, Rc<Block<I>>>,
    /// Block currently being recorded. `None` when not in a recording pass.
    recording: Option<(BlockKey, Block<I>)>,
}

/// Classify a guest PC by memory region for the block-cache split.
/// See GBA memory map in resources/gbatek: BIOS is 0x0000_0000-0x0000_3FFF,
/// ROM (WS0/1/2) at 0x0800_0000-0x0DFF_FFFF.
#[inline]
fn is_rom_address(pc: u32) -> bool {
    let top = pc & 0xff00_0000;
    // BIOS range (top byte 0x00) or any cartridge wait-state region.
    top == 0x0000_0000 || (0x0800_0000..=0x0d00_0000).contains(&top)
}

impl<I: MemoryInterface> Default for BlockCache<I> {
    fn default() -> Self {
        BlockCache {
            rom_blocks: HashMap::with_capacity(2048),
            ram_blocks: HashMap::with_capacity(256),
            recording: None,
        }
    }
}

impl<I: MemoryInterface> BlockCache<I> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn get(&self, key: BlockKey) -> Option<Rc<Block<I>>> {
        let pc = key.0 & !1;
        if is_rom_address(pc) {
            self.rom_blocks.get(&key).cloned()
        } else {
            self.ram_blocks.get(&key).cloned()
        }
    }

    /// Called on any RAM write to invalidate RAM-region blocks. ROM-region
    /// blocks are left alone — no write can reach that memory anyway.
    #[inline]
    pub fn flush(&mut self) {
        self.ram_blocks.clear();
        // If we happen to be mid-recording a RAM-region block, drop it too.
        // ROM recordings stay valid.
        if let Some((key, _)) = &self.recording
            && !is_rom_address(key.0 & !1)
        {
            self.recording = None;
        }
    }

    /// Flush everything (ROM and RAM). Used only by diagnostics/tests; normal
    /// invalidation goes through `flush()` which preserves ROM entries.
    #[allow(dead_code)]
    #[inline]
    pub fn flush_all(&mut self) {
        self.rom_blocks.clear();
        self.ram_blocks.clear();
        self.recording = None;
    }

    /// Begin trace-recording a new block starting at `key`.
    #[inline]
    pub fn begin_record(&mut self, key: BlockKey) {
        self.recording = Some((key, Block::new()));
    }

    /// Append one executed instruction to the in-progress recording, if any.
    #[inline]
    pub fn record_instr(&mut self, instr: DecodedInstr<I>) {
        if let Some((_, block)) = &mut self.recording {
            // Cap block length to avoid runaway recordings on pathological code
            // paths (long loops that never pipeline-flush would just trace forever).
            // 64 is arbitrary but comfortably larger than typical basic blocks.
            if block.instrs.len() < 64 {
                block.instrs.push(instr);
            }
        }
    }

    /// Finish the current recording and insert it into the appropriate cache
    /// half based on the block's entry region. Called on pipeline flush or
    /// when the block length cap is reached.
    #[inline]
    pub fn finish_record(&mut self) {
        if let Some((key, block)) = self.recording.take()
            && !block.instrs.is_empty()
        {
            let pc = key.0 & !1;
            if is_rom_address(pc) {
                self.rom_blocks.insert(key, Rc::new(block));
            } else {
                self.ram_blocks.insert(key, Rc::new(block));
            }
        }
    }

    /// Drop the in-progress recording without saving. Used when we bail out of
    /// a partial recording (e.g. we were mid-block when the caller decided to
    /// invalidate).
    #[allow(dead_code)]
    #[inline]
    pub fn abort_record(&mut self) {
        self.recording = None;
    }

    #[inline]
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    #[allow(dead_code)]
    #[inline]
    pub fn len(&self) -> usize {
        self.rom_blocks.len() + self.ram_blocks.len()
    }
}
