use std::num::NonZeroU64;

use crate::alloc::{Buffer, Meter};
use crate::{LimitExceeded, NodeFieldId, SkipPolicy};

/// Lengths of the output tape's buffers. Restoring a mark rolls back every
/// operation recorded after it in O(1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TapeMark {
    pub ops: usize,
    pub nodes: usize,
    pub inspection: usize,
}

/// Effect-control depths restored together on backtracking.
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
    /// Callee entry (raw code address).
    pub target: u16,
    /// Executor-owned token identifying the call's return map.
    pub call_site: u16,
    /// Field constraint the next candidate must satisfy, if any.
    pub field: Option<NodeFieldId>,
    /// How to advance to the next candidate.
    pub policy: SkipPolicy,
}

/// What backtracking does after restoring a checkpoint's state.
#[derive(Clone, Copy, Debug)]
pub enum Resume {
    /// Resume dispatch at the checkpoint's successor address.
    Successor,
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
/// by both successor and Call-retry checkpoints. Bundling these fields keeps the
/// snapshot at creation and the restore on backtrack in lockstep.
#[derive(Clone, Copy, Debug)]
pub struct CheckpointState {
    /// Cursor position (tree-sitter descendant_index) — always present; the
    /// restore fallback when the pooled snapshot was evicted.
    pub descendant_index: u32,
    /// Output tape lengths at checkpoint.
    pub tape: TapeMark,
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
    /// Successor resume target, or the owning instruction's own address for
    /// Call/Match retries (re-decoded on resume; also used for tracing).
    pub ip: u16,
    /// How backtracking continues after restoring `state`.
    pub resume: Resume,
    pub(crate) max_frame_idx_below: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct CheckpointSnapshot {
    stack_index: usize,
    snapshot: NonZeroU64,
}

impl Checkpoint {
    /// Create a checkpoint that resumes at successor address `ip`.
    pub fn successor(state: CheckpointState, ip: u16) -> Self {
        Self {
            state,
            ip,
            resume: Resume::Successor,
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
pub struct CheckpointStack {
    stack: Buffer<Checkpoint>,
    snapshots: Buffer<CheckpointSnapshot>,
    max_frame_idx: Option<u32>,
}

impl CheckpointStack {
    pub fn new() -> Self {
        Self {
            stack: Buffer::new(),
            snapshots: Buffer::new(),
            max_frame_idx: None,
        }
    }

    #[cfg(feature = "execution")]
    pub(crate) fn clear(&mut self) {
        self.stack.clear();
        self.snapshots.clear();
        self.max_frame_idx = None;
    }

    pub fn push(
        &mut self,
        meter: &mut Meter<'_>,
        mut checkpoint: Checkpoint,
    ) -> Result<(), LimitExceeded> {
        let max_frame_idx = self.prefix_max(&mut checkpoint);
        self.stack.push(meter, checkpoint)?;
        self.max_frame_idx = max_frame_idx;
        Ok(())
    }

    pub fn push_with_snapshot(
        &mut self,
        meter: &mut Meter<'_>,
        mut checkpoint: Checkpoint,
        snapshot: Option<NonZeroU64>,
    ) -> Result<(), LimitExceeded> {
        let Some(snapshot) = snapshot else {
            return self.push(meter, checkpoint);
        };
        let stack_index = self.stack.len();
        let max_frame_idx = self.prefix_max(&mut checkpoint);
        self.snapshots.reserve(meter, 1)?;
        self.stack.push(meter, checkpoint)?;
        self.snapshots.push(
            meter,
            CheckpointSnapshot {
                stack_index,
                snapshot,
            },
        )?;
        self.max_frame_idx = max_frame_idx;
        Ok(())
    }

    /// Ensure room for `additional` more checkpoints, with snapshot metadata
    /// when `snapshots` is set, so the pushes that follow cannot fail.
    pub fn reserve(
        &mut self,
        meter: &mut Meter<'_>,
        additional: usize,
        snapshots: bool,
    ) -> Result<bool, LimitExceeded> {
        self.stack.reserve(meter, additional)?;
        if snapshots {
            if self
                .snapshots
                .capacity_bytes()
                .saturating_add(self.snapshots.reservation_bytes(additional)?)
                > 32 * 1024
            {
                return Ok(false);
            }

            match self.snapshots.reserve(meter, additional) {
                Ok(()) => return Ok(true),
                Err(LimitExceeded::Memory { .. } | LimitExceeded::Allocation { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }

    fn prefix_max(&self, checkpoint: &mut Checkpoint) -> Option<u32> {
        let prev = self.stack.last().and_then(|c| c.max_frame_idx_below);
        checkpoint.max_frame_idx_below = match (checkpoint.state.frame_index, prev) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        checkpoint.max_frame_idx_below
    }

    pub fn pop(&mut self) -> Option<Checkpoint> {
        assert!(
            self.snapshots.is_empty(),
            "checkpoint stack used `pop` while {} cursor snapshot metadata entries remain; \
             snapshot-enabled execution must use `pop_with_snapshot`",
            self.snapshots.len()
        );
        let cp = self.stack.pop()?;
        self.max_frame_idx = self.stack.last().and_then(|c| c.max_frame_idx_below);
        Some(cp)
    }

    pub fn pop_with_snapshot(&mut self) -> Option<(Checkpoint, Option<NonZeroU64>)> {
        let Some(stack_index) = self.stack.len().checked_sub(1) else {
            assert!(
                self.snapshots.is_empty(),
                "checkpoint stack is empty, but {} cursor snapshot metadata entries remain; \
                 snapshot metadata must be released with its checkpoint",
                self.snapshots.len()
            );
            return None;
        };
        if let Some(snapshot) = self
            .snapshots
            .last()
            .filter(|snapshot| snapshot.stack_index > stack_index)
        {
            panic!(
                "cursor snapshot metadata points above the checkpoint stack: \
                 snapshot_stack_index={}, top_checkpoint_index={stack_index}, snapshot_id={}",
                snapshot.stack_index, snapshot.snapshot
            );
        }
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

    #[inline]
    pub fn max_frame_idx(&self) -> Option<u32> {
        self.max_frame_idx
    }
}

impl Default for CheckpointStack {
    fn default() -> Self {
        Self::new()
    }
}
