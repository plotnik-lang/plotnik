//! TreeCursor wrapper with Plotnik navigation semantics.
//!
//! The wrapper handles the search loop and skip policies defined
//! in docs/tree-navigation.md.

use std::collections::VecDeque;
use std::num::NonZeroU64;

use tree_sitter::{Node, TreeCursor};

use crate::{Nav, NodeClass, NodeFieldId, SkipClass, SkipPolicy};

/// Upper bound on live snapshots. Restores overwhelmingly hit the newest
/// checkpoints (LIFO unwinding), so a small window captures nearly all hits
/// while bounding memory to CAP cursors regardless of checkpoint-stack depth.
const SNAPSHOT_CAP: usize = 64;

/// Flat per-cursor estimate for the memory ceiling: a cursor's heap is its
/// entry stack (~28 bytes per tree level, capacity high-watered). 2 KiB covers
/// ~70 levels; deeper trees under-account, but the CAP bounds the absolute
/// error to noise against the >=64 MiB ceiling.
const SNAPSHOT_FOOTPRINT_ESTIMATE: u64 = 2048;

/// Snapshot creation is lazy because match-heavy workloads create many
/// checkpoints that restore in-place or move only a few nodes. Storms are
/// distinguished by repeated wide lateral jumps, then get the O(depth)
/// snapshot path for the hot part of the run.
const SNAPSHOT_ACTIVATION_WIDE_MISSES: u32 = 32;

/// Below this distance, `goto_descendant` is cheap enough that snapshotting at
/// creation costs more than it saves. The storm probe averaged 64 descendant
/// indices per non-same restore; match-heavy workloads stayed near zero.
const SNAPSHOT_ACTIVATION_MIN_JUMP: u32 = 32;

impl SkipPolicy {
    /// Whether a sibling search under this policy may step over `node` — both
    /// when scanning forward past a rejected candidate and when resuming past
    /// an accepted-but-failed one (the node then sits in the pattern's gap,
    /// which must admit it).
    pub fn admits(self, node: &Node<'_>) -> bool {
        self.skip_class().admits(CursorWrapper::node_class(node))
    }
}

/// Exit constraint for Up navigation, checked at *each* level ascended (so
/// same-mode `Up*` instructions compose; see [`CursorWrapper::go_up`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpMode {
    /// No constraint - just ascend.
    Any,
    /// Each node left must be its parent's last non-trivia child.
    SkipTrivia,
    /// Each node left must be its parent's last non-extra child.
    SkipExtras,
    /// Each node left must be its parent's last child.
    Exact,
}

impl UpMode {
    fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::SkipTrivia => SkipClass::Trivia,
            Self::SkipExtras => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }
}

/// Wrapper around TreeCursor with Plotnik navigation semantics.
///
/// Critical: The cursor is created at tree root; pooled snapshots are clones of
/// that root-created cursor, and `reset_to` copies the root along with the
/// stack. Every cursor in play therefore shares a root, so `descendant_index`
/// stays root-relative for O(1) checkpoint saves and O(depth) restores.
pub struct CursorWrapper<'t> {
    cursor: TreeCursor<'t>,
    snapshots: SnapshotPool<'t>,
}

/// Pool of cursor snapshots keyed by a monotonically increasing sequence
/// number. `live` is ordered oldest to newest (seq strictly increasing), so the
/// newest snapshot is at the back, where LIFO restores look. Evicted or
/// consumed cursors go to `free` and are recycled by `reset_to`, so the pool
/// stops allocating once warm.
pub(crate) struct SnapshotPool<'t> {
    live: VecDeque<SnapshotEntry<'t>>,
    free: Vec<TreeCursor<'t>>,
    /// u64: one seq per checkpoint push once active — a u32 could plausibly
    /// wrap within a long unlimited-budget run and panic mid-query.
    next_seq: u64,
    wide_restore_misses: u32,
}

struct SnapshotEntry<'t> {
    seq: NonZeroU64,
    /// Checkpoints on the stack still holding this seq. A branch fan-out shares
    /// one snapshot across all its alternative checkpoints.
    refs: u32,
    cursor: TreeCursor<'t>,
}

impl<'t> SnapshotPool<'t> {
    fn new() -> Self {
        Self {
            live: VecDeque::new(),
            free: Vec::new(),
            next_seq: 0,
            wide_restore_misses: 0,
        }
    }

    /// Bytes charged against the VM memory ceiling (estimate; see constant).
    fn byte_footprint(&self) -> u64 {
        (self.live.len() + self.free.len()) as u64 * SNAPSHOT_FOOTPRINT_ESTIMATE
    }
}

impl<'t> CursorWrapper<'t> {
    pub fn new(cursor: TreeCursor<'t>) -> Self {
        Self {
            cursor,
            snapshots: SnapshotPool::new(),
        }
    }

    #[inline]
    pub fn node(&self) -> Node<'t> {
        self.cursor.node()
    }

    #[inline]
    pub fn descendant_index(&self) -> u32 {
        self.cursor.descendant_index() as u32
    }

    #[inline]
    pub fn goto_descendant(&mut self, index: u32) {
        self.cursor.goto_descendant(index as usize);
    }

    /// Snapshot the current position for `refs` checkpoints about to be pushed.
    /// Costs one clone the first few times, then one `reset_to` (O(depth) memcpy)
    /// per call.
    #[inline]
    pub fn snapshot(&mut self, refs: u32) -> Option<NonZeroU64> {
        if !self.snapshots_active() {
            return None;
        }
        assert!(refs > 0, "snapshot needs at least one checkpoint reference");

        let pool = &mut self.snapshots;
        pool.next_seq = pool
            .next_seq
            .checked_add(1)
            .expect("snapshot sequence overflow");
        let seq = NonZeroU64::new(pool.next_seq).expect("seq starts at 1");
        let cursor = match pool.free.pop() {
            Some(mut cursor) => {
                cursor.reset_to(&self.cursor);
                cursor
            }
            None => self.cursor.clone(),
        };

        pool.live.push_back(SnapshotEntry { seq, refs, cursor });
        if pool.live.len() > SNAPSHOT_CAP {
            let evicted = pool.live.pop_front().expect("len > CAP > 0");
            pool.free.push(evicted.cursor);
        }

        Some(seq)
    }

    /// Restore to a checkpoint position, using its pooled snapshot when it
    /// survived eviction. Always releases the checkpoint's reference on a hit.
    #[inline(always)]
    pub fn restore(&mut self, snapshot: Option<NonZeroU64>, index: u32) {
        let Some(seq) = snapshot else {
            self.restore_without_snapshot(index);
            return;
        };
        self.restore_snapshot(seq, index);
    }

    #[inline(always)]
    pub fn restore_without_snapshot(&mut self, index: u32) -> bool {
        let current_index = self.cursor.descendant_index() as u32;
        if current_index == index {
            return false;
        }
        self.restore_to_from_moved(current_index, index)
    }

    #[inline(always)]
    fn restore_snapshot(&mut self, seq: NonZeroU64, index: u32) {
        let current_index = self.descendant_index();
        if self.consume_snapshot(seq, index, current_index) {
            return;
        }
        if current_index == index {
            return;
        }
        self.restore_to_from_moved(current_index, index);
    }

    /// Bytes charged against the VM memory ceiling for pooled cursor snapshots.
    #[inline]
    pub fn snapshot_footprint(&self) -> u64 {
        self.snapshots.byte_footprint()
    }

    /// True if the snapshot was found and the cursor restored from it.
    fn consume_snapshot(&mut self, seq: NonZeroU64, index: u32, current_index: u32) -> bool {
        let cursor_is_at_index = current_index == index;
        let pool = &mut self.snapshots;

        // Anything newer than `seq` belongs to checkpoints already popped
        // (LIFO); their refs must have hit zero. Drain defensively so a
        // bookkeeping slip degrades to fallback instead of a wrong restore.
        while let Some(back) = pool.live.back()
            && back.seq > seq
        {
            debug_assert_eq!(
                back.refs, 0,
                "snapshot newer than the checkpoint being restored still referenced"
            );
            let entry = pool.live.pop_back().expect("back exists");
            pool.free.push(entry.cursor);
        }

        let Some(back) = pool.live.back_mut() else {
            return false;
        };
        if back.seq != seq {
            return false;
        }

        if !cursor_is_at_index {
            self.cursor.reset_to(&back.cursor);
            debug_assert_eq!(
                self.cursor.descendant_index() as u32,
                index,
                "pooled snapshot restored the wrong cursor position"
            );
        }

        back.refs = back
            .refs
            .checked_sub(1)
            .expect("snapshot reference released more times than acquired");
        if back.refs == 0 {
            let entry = pool.live.pop_back().expect("back exists");
            pool.free.push(entry.cursor);
        }

        true
    }

    #[inline]
    fn restore_to_from_moved(&mut self, current_index: u32, index: u32) -> bool {
        let mut activated = false;
        if current_index.abs_diff(index) >= SNAPSHOT_ACTIVATION_MIN_JUMP {
            activated = self.record_wide_restore_miss();
        }
        self.cursor.goto_descendant(index as usize);
        activated
    }

    #[inline]
    pub fn snapshots_active(&self) -> bool {
        self.snapshots.wide_restore_misses >= SNAPSHOT_ACTIVATION_WIDE_MISSES
    }

    fn record_wide_restore_miss(&mut self) -> bool {
        if self.snapshots.wide_restore_misses >= SNAPSHOT_ACTIVATION_WIDE_MISSES {
            return false;
        }
        self.snapshots.wide_restore_misses += 1;
        self.snapshots.wide_restore_misses == SNAPSHOT_ACTIVATION_WIDE_MISSES
    }

    #[inline]
    pub fn field_id(&self) -> Option<NodeFieldId> {
        self.cursor.field_id().map(NodeFieldId::from)
    }

    #[inline]
    fn node_class(node: &Node<'_>) -> NodeClass {
        NodeClass {
            anonymous: !node.is_named(),
            extra: node.is_extra(),
        }
    }

    #[inline]
    pub fn is_trivia(node: &Node<'_>) -> bool {
        SkipClass::Trivia.admits(Self::node_class(node))
    }

    /// Navigate according to Nav command, preparing for match attempt.
    ///
    /// Returns the skip policy to use for the subsequent match attempt
    /// ([`Nav::skip_policy`] — the single source of the nav→policy mapping),
    /// or None if navigation failed (no children/siblings).
    pub fn navigate(&mut self, nav: Nav) -> Option<SkipPolicy> {
        let moved = match nav {
            // Epsilon should never reach here - VM skips navigate for epsilon
            Nav::Epsilon => {
                debug_assert!(
                    false,
                    "navigate called with Epsilon - should be skipped by VM"
                );
                true
            }
            Nav::Stay | Nav::StayExact => true,
            Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact => {
                self.go_first_child()
            }
            Nav::Next | Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact => {
                self.go_next_sibling()
            }
            Nav::ChildlessSkipTrivia => self.childless_holds(SkipClass::Trivia),
            Nav::ChildlessSkipExtras => self.childless_holds(SkipClass::Extras),
            Nav::ChildlessExact => self.childless_holds(SkipClass::Exact),
            Nav::Up(n) => self.go_up(n, UpMode::Any),
            Nav::UpSkipTrivia(n) => self.go_up(n, UpMode::SkipTrivia),
            Nav::UpSkipExtras(n) => self.go_up(n, UpMode::SkipExtras),
            Nav::UpExact(n) => self.go_up(n, UpMode::Exact),
        };
        moved.then_some(nav.skip_policy())
    }

    fn go_first_child(&mut self) -> bool {
        self.cursor.goto_first_child()
    }

    /// Whether every child of the current node is `skip_class`-skippable —
    /// i.e. the node is childless once trivia/extras are ignored (`Exact`
    /// admits nothing, so it requires true childlessness). The child scan is
    /// undone before returning, so the cursor stays on the current node.
    fn childless_holds(&mut self, skip_class: SkipClass) -> bool {
        let origin = self.descendant_index();
        if !self.cursor.goto_first_child() {
            return true;
        }
        loop {
            if !skip_class.admits(Self::node_class(&self.cursor.node())) {
                self.goto_descendant(origin);
                return false;
            }
            if !self.cursor.goto_next_sibling() {
                break;
            }
        }
        self.goto_descendant(origin);
        true
    }

    fn go_next_sibling(&mut self) -> bool {
        self.cursor.goto_next_sibling()
    }

    /// Ascend `levels` levels, validating the exit constraint at *every* level.
    ///
    /// Same-mode `Up*` instructions compose: `Up*(a)` then `Up*(b)` is `Up*(a+b)`,
    /// because each node being exited is checked in turn. This is what makes a
    /// nested trailing anchor — `(array (object (pair) .) .)`, "pair last in
    /// object AND object last in array" — sound when `collapse_up` merges the two
    /// single-level ascents (and why merging caps at the encoding limit rather
    /// than dropping checks; see docs/tree-navigation.md).
    ///
    /// On any failure the cursor is restored to where it started, so a failed
    /// navigation leaves no net movement (the VM also backtracks to a checkpoint,
    /// but keeping this self-contained avoids relying on that).
    fn go_up(&mut self, levels: u8, mode: UpMode) -> bool {
        let origin = self.descendant_index();
        for _ in 0..levels {
            if !self.exit_constraint_holds(mode) || !self.cursor.goto_parent() {
                self.goto_descendant(origin);
                return false;
            }
        }
        true
    }

    /// Whether the current node satisfies `mode`'s last-child constraint. Any
    /// sibling probe is undone before returning, so the cursor stays on the
    /// current node either way — ready for the caller to ascend.
    fn exit_constraint_holds(&mut self, mode: UpMode) -> bool {
        match mode {
            UpMode::Any => true,
            UpMode::Exact => {
                // Must be the last child — no next sibling at all. The probe is
                // undone with goto_descendant, not goto_previous_sibling: the
                // latter can rebuild the cursor entry with an off-by-one
                // descendant index when a hidden supertype sits between parent
                // and child, silently skewing every index-based restore after it.
                let saved = self.descendant_index();
                if self.cursor.goto_next_sibling() {
                    self.goto_descendant(saved);
                    return false;
                }
                true
            }
            // Last child once trailing trivia / extras are ignored.
            UpMode::SkipTrivia => self.is_last_child_skipping(Self::is_trivia),
            UpMode::SkipExtras => {
                let skip_class = mode.skip_class();
                self.is_last_child_skipping(|n| skip_class.admits(Self::node_class(n)))
            }
        }
    }

    /// Whether no non-`skippable` sibling follows the current node — i.e. it is
    /// the last child once trailing `skippable` siblings are ignored. The sibling
    /// scan is undone before returning, so the cursor stays on the current node.
    fn is_last_child_skipping(&mut self, skippable: impl Fn(&Node<'t>) -> bool) -> bool {
        let saved = self.descendant_index();
        while self.cursor.goto_next_sibling() {
            if !skippable(&self.cursor.node()) {
                self.goto_descendant(saved);
                return false;
            }
        }
        self.goto_descendant(saved);
        true
    }

    /// Continue searching for a match with the given skip policy.
    ///
    /// This is called when a match attempt fails. It advances to the next
    /// sibling based on the skip policy and returns whether to retry.
    ///
    /// - `Exact`: Return false (no skipping allowed)
    /// - `Trivia`: Skip trivia siblings only, fail if non-trivia
    /// - `Extras`: Skip tree-sitter extras only, fail otherwise
    /// - `Any`: Skip any siblings
    pub fn continue_search(&mut self, policy: SkipPolicy) -> bool {
        match policy {
            SkipPolicy::Exact => false,
            SkipPolicy::Trivia | SkipPolicy::Extras => {
                if !policy.admits(&self.cursor.node()) {
                    return false;
                }
                self.cursor.goto_next_sibling()
            }
            SkipPolicy::Any => self.cursor.goto_next_sibling(),
        }
    }
}
