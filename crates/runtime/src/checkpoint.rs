use std::num::NonZeroU64;

use crate::{NodeFieldId, SkipPolicy};

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

/// The execution state a checkpoint snapshots and later restores. Bundling these
/// fields keeps the snapshot at creation and the restore on backtrack in lockstep.
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
pub struct VmResume {
    /// Successor resume target, or the owning instruction's own address for
    /// Call/Match retries (re-decoded on resume; also used for tracing).
    pub ip: u16,
    /// How VM backtracking continues after restoring checkpoint state.
    pub resume: Resume,
}

impl From<u16> for VmResume {
    fn from(ip: u16) -> Self {
        Self {
            ip,
            resume: Resume::Successor,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Checkpoint<R = VmResume> {
    /// Execution state to restore on backtrack.
    pub state: CheckpointState,
    /// Executor-owned continuation: VM retry information or a generated block.
    pub resume: R,
    pub(crate) max_frame_idx_below: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct CheckpointSnapshot {
    stack_index: usize,
    snapshot: NonZeroU64,
}

impl<R> Checkpoint<R> {
    pub fn new(state: CheckpointState, resume: R) -> Self {
        Self {
            state,
            resume,
            max_frame_idx_below: None,
        }
    }
}

impl Checkpoint<VmResume> {
    /// Create a Call retry checkpoint that advances and re-enters the callee.
    /// `call_ip` is the Call's own address, retained only for trace rendering;
    /// re-entry is driven entirely by `call_resume`.
    pub fn call_retry(state: CheckpointState, call_ip: u16, call_resume: CallResume) -> Self {
        Self::new(
            state,
            VmResume {
                ip: call_ip,
                resume: Resume::Call(call_resume),
            },
        )
    }

    /// Create a Match retry checkpoint: `state` snapshots the accepted
    /// candidate, `match_ip` addresses the owning Match instruction. On
    /// backtrack the engine advances past the candidate and re-runs the
    /// match's sibling search from there.
    pub fn match_retry(state: CheckpointState, match_ip: u16) -> Self {
        Self::new(
            state,
            VmResume {
                ip: match_ip,
                resume: Resume::Match,
            },
        )
    }
}

/// Stack of checkpoints with O(1) `max_frame_idx` tracking.
///
/// The `max_frame_idx` is maintained for frame arena pruning: we track the
/// highest frame index referenced by any checkpoint so pruning knows which
/// frames are safe to remove. It is kept current through each checkpoint's
/// `max_frame_idx_below` prefix-max rather than recomputed on backtrack.
#[derive(Debug)]
pub struct CheckpointStack<R = VmResume> {
    stack: Vec<Checkpoint<R>>,
    snapshots: Vec<CheckpointSnapshot>,
    max_frame_idx: Option<u32>,
}

impl<R> CheckpointStack<R> {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            snapshots: Vec::new(),
            max_frame_idx: None,
        }
    }

    #[cfg(feature = "execution")]
    pub(crate) fn clear(&mut self) {
        self.stack.clear();
        self.snapshots.clear();
        self.max_frame_idx = None;
    }

    pub fn push(&mut self, mut checkpoint: Checkpoint<R>) {
        self.push_inner(&mut checkpoint);
        self.stack.push(checkpoint);
    }

    pub fn push_with_snapshot(&mut self, mut checkpoint: Checkpoint<R>, snapshot: NonZeroU64) {
        let stack_index = self.stack.len();
        self.push_inner(&mut checkpoint);
        self.stack.push(checkpoint);
        self.snapshots.push(CheckpointSnapshot {
            stack_index,
            snapshot,
        });
    }

    fn push_inner(&mut self, checkpoint: &mut Checkpoint<R>) {
        let prev = self.stack.last().and_then(|c| c.max_frame_idx_below);
        checkpoint.max_frame_idx_below = match (checkpoint.state.frame_index, prev) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.max_frame_idx = checkpoint.max_frame_idx_below;
    }

    pub fn pop(&mut self) -> Option<Checkpoint<R>> {
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

    pub fn pop_with_snapshot(&mut self) -> Option<(Checkpoint<R>, Option<NonZeroU64>)> {
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

    /// Occupied checkpoint and snapshot-metadata bytes, excluding spare capacity.
    #[inline]
    pub fn byte_footprint(&self) -> u64 {
        (self.stack.len() * std::mem::size_of::<Checkpoint<R>>()
            + self.snapshots.len() * std::mem::size_of::<CheckpointSnapshot>()) as u64
    }
}

impl<R> Default for CheckpointStack<R> {
    fn default() -> Self {
        Self::new()
    }
}
