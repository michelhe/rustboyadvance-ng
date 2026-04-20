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

use crate::cpu::{Arm7tdmiCore, CpuAction};
use crate::memory::MemoryInterface;

/// One recorded instruction inside a block.
///
/// Handler fn signatures differ between ARM and Thumb (u32 vs u16 insn word),
/// so we encode the mode in an enum. A uniform `u32`-argument handler would
/// require trampolines; this enum costs one jump-table branch per instruction
/// but keeps the handlers untouched.
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
/// Empty in the default build — the struct and all its methods compile to
/// no-ops unless the `cached_interp` feature is on.
pub struct BlockCache<I: MemoryInterface> {
    blocks: HashMap<BlockKey, Block<I>>,
    /// Block currently being recorded. `None` when not in a recording pass.
    recording: Option<(BlockKey, Block<I>)>,
}

impl<I: MemoryInterface> Default for BlockCache<I> {
    fn default() -> Self {
        BlockCache {
            blocks: HashMap::with_capacity(1024),
            recording: None,
        }
    }
}

impl<I: MemoryInterface> BlockCache<I> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn get(&self, key: BlockKey) -> Option<&Block<I>> {
        self.blocks.get(&key)
    }

    /// Called on any RAM write to invalidate cached blocks. This is the coarse
    /// first-pass policy: flush everything. A finer per-page scheme can slot in
    /// here later without changing callers.
    #[inline]
    pub fn flush(&mut self) {
        self.blocks.clear();
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

    /// Finish the current recording and insert it into the cache. Called on
    /// pipeline flush or when the block length cap is reached.
    #[inline]
    pub fn finish_record(&mut self) {
        if let Some((key, block)) = self.recording.take()
            && !block.instrs.is_empty()
        {
            self.blocks.insert(key, block);
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
}
