//! Checkpoints for backtracking.
//!
//! When the VM encounters a branch (multiple successors), it saves
//! a checkpoint for each alternative. On failure, it restores the
//! most recent checkpoint and continues.

use std::num::NonZeroU64;

use crate::{NodeFieldId, SkipPolicy};

/// Effect-control depths restored together on backtracking.
///
/// Both counters are bounded by the `u32` call/effect structure, so two lanes
/// retain the post-`u16` suppression range without padding every checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectDepths {
    suppression: u32,
    scalar: u32,
}

impl EffectDepths {
    pub fn new(suppression: u32, scalar: u32) -> Self {
        Self {
            suppression,
            scalar,
        }
    }

    pub fn suppression(self) -> u32 {
        self.suppression
    }

    pub fn scalar(self) -> u32 {
        self.scalar
    }
}

/// Everything needed to re-enter a callee at the next sibling after a Call's
/// callee fails. Carrying this on the checkpoint (rather than in ambient VM
/// state) keeps the resume fully self-contained: `backtrack` advances the
/// cursor and re-enters the callee without re-running the Call's navigation.
#[derive(Clone, Copy, Debug)]
pub struct CallResume {
    /// Callee entry (raw step index).
    pub target: u16,
    /// Return address after the callee (raw step index).
    pub next: u16,
    /// Field constraint the next candidate must satisfy, if any.
    pub field: Option<NodeFieldId>,
    /// How to advance to the next candidate.
    pub policy: SkipPolicy,
}

/// What backtracking does after restoring a checkpoint's state. Every point
/// with alternatives leaves a checkpoint whose resume says how to take the
/// next one — branch alternatives jump, sibling searches (Match or Call)
/// advance the cursor and re-try. This uniform discipline is what makes every
/// candidate acceptance revisitable; a search with no live resume checkpoint
/// would silently commit (the historical sibling-retry hole).
#[derive(Clone, Copy, Debug)]
pub enum Resume {
    /// Plain branch alternative: resume dispatch at the checkpoint's `ip`.
    Branch,
    /// Call retry: advance the cursor to the next admissible candidate and
    /// re-enter the callee, without re-running the Call's navigation.
    Call(CallResume),
    /// Match retry: the checkpoint's `ip` addresses a Match whose sibling
    /// search accepted the checkpointed candidate. Advance past it (per the
    /// nav's skip policy, re-derived from the instruction) and re-run the
    /// candidate search from there.
    Match,
}

/// The VM state a checkpoint snapshots and later restores: everything shared
/// by both branch and Call-retry checkpoints. Bundling these fields keeps the
/// snapshot at creation and the restore on backtrack in lockstep.
#[derive(Clone, Copy, Debug)]
pub struct CheckpointState {
    /// Cursor position (tree-sitter descendant_index) — always present; the
    /// restore fallback when the pooled snapshot was evicted.
    pub descendant_index: u32,
    /// Match journal length at checkpoint.
    pub journal_watermark: usize,
    /// Frame arena state at checkpoint.
    pub frame_index: Option<u32>,
    /// Recursion depth at checkpoint.
    pub recursion_depth: u32,
    /// Suppression and open scalar-frame depths.
    pub effect_depths: EffectDepths,
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    /// VM state to restore on backtrack.
    pub state: CheckpointState,
    /// Branch resume target, or the owning instruction's own address for
    /// Call/Match retries (re-decoded on resume; also used for tracing).
    pub ip: u16,
    /// How backtracking continues after restoring `state`.
    pub resume: Resume,
    /// Maximum `frame_index` over this checkpoint and everything beneath it on
    /// the stack. The whole stack's max is therefore the top's `max_frame_idx_below`,
    /// so pruning never has to scan.
    pub(crate) max_frame_idx_below: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct CheckpointSnapshot {
    stack_index: usize,
    snapshot: NonZeroU64,
}

impl Checkpoint {
    /// Create a plain (branch alternative) checkpoint that resumes at `ip`.
    pub fn branch(state: CheckpointState, ip: u16) -> Self {
        Self {
            state,
            ip,
            resume: Resume::Branch,
            max_frame_idx_below: None,
        }
    }

    /// Create a Call retry checkpoint that advances and re-enters the callee.
    /// `call_ip` is the Call's own address, retained only for trace rendering;
    /// re-entry is driven entirely by `call_resume`.
    pub fn call_retry(state: CheckpointState, call_ip: u16, call_resume: CallResume) -> Self {
        Self {
            state,
            ip: call_ip,
            resume: Resume::Call(call_resume),
            max_frame_idx_below: None,
        }
    }

    /// Create a Match retry checkpoint: `state` snapshots the accepted
    /// candidate, `match_ip` addresses the owning Match instruction. On
    /// backtrack the engine advances past the candidate and re-runs the
    /// match's sibling search from there.
    pub fn match_retry(state: CheckpointState, match_ip: u16) -> Self {
        Self {
            state,
            ip: match_ip,
            resume: Resume::Match,
            max_frame_idx_below: None,
        }
    }
}

/// Stack of checkpoints with O(1) `max_frame_idx` tracking.
///
/// The `max_frame_idx` is maintained for frame arena pruning: we track the
/// highest frame index referenced by any checkpoint so pruning knows which
/// frames are safe to remove. It is kept current through each checkpoint's
/// `max_frame_idx_below` prefix-max rather than recomputed on backtrack.
#[derive(Debug)]
pub struct CheckpointStack {
    stack: Vec<Checkpoint>,
    snapshots: Vec<CheckpointSnapshot>,
    /// Highest frame index referenced by any checkpoint.
    max_frame_idx: Option<u32>,
}

impl CheckpointStack {
    /// Create an empty checkpoint stack.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            snapshots: Vec::new(),
            max_frame_idx: None,
        }
    }

    pub fn push(&mut self, mut checkpoint: Checkpoint) {
        self.push_inner(&mut checkpoint);
        self.stack.push(checkpoint);
    }

    pub fn push_with_snapshot(&mut self, mut checkpoint: Checkpoint, snapshot: NonZeroU64) {
        let stack_index = self.stack.len();
        self.push_inner(&mut checkpoint);
        self.stack.push(checkpoint);
        self.snapshots.push(CheckpointSnapshot {
            stack_index,
            snapshot,
        });
    }

    fn push_inner(&mut self, checkpoint: &mut Checkpoint) {
        let prev = self.stack.last().and_then(|c| c.max_frame_idx_below);
        checkpoint.max_frame_idx_below = match (checkpoint.state.frame_index, prev) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.max_frame_idx = checkpoint.max_frame_idx_below;
    }

    pub fn pop(&mut self) -> Option<Checkpoint> {
        let cp = self.stack.pop()?;
        debug_assert!(
            self.snapshots.is_empty(),
            "snapshot-aware pop is required once snapshots exist"
        );
        self.max_frame_idx = self.stack.last().and_then(|c| c.max_frame_idx_below);
        Some(cp)
    }

    pub fn pop_with_snapshot(&mut self) -> Option<(Checkpoint, Option<NonZeroU64>)> {
        let stack_index = self.stack.len().checked_sub(1)?;
        let cp = self.stack.pop()?;
        let snapshot = if self
            .snapshots
            .last()
            .is_some_and(|snapshot| snapshot.stack_index == stack_index)
        {
            Some(
                self.snapshots
                    .pop()
                    .expect("snapshot entry exists")
                    .snapshot,
            )
        } else {
            None
        };
        self.max_frame_idx = self.stack.last().and_then(|c| c.max_frame_idx_below);
        Some((cp, snapshot))
    }

    /// Get the highest frame index referenced by any checkpoint.
    #[inline]
    pub fn max_frame_idx(&self) -> Option<u32> {
        self.max_frame_idx
    }

    /// Live heap bytes: checkpoint count × checkpoint size.
    #[inline]
    pub fn byte_footprint(&self) -> u64 {
        (self.stack.len() * std::mem::size_of::<Checkpoint>()
            + self.snapshots.len() * std::mem::size_of::<CheckpointSnapshot>()) as u64
    }
}

impl Default for CheckpointStack {
    fn default() -> Self {
        Self::new()
    }
}
