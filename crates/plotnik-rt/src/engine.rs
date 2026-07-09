//! Shared execution-state core for query executors.
//!
//! [`Engine`] bundles the mutable state a query run threads through every
//! instruction — cursor, call frames, backtracking checkpoints, the capture
//! effect log, and the suppression gate — together with the transitions that
//! must stay in lockstep: checkpoint save/restore, frame entry/exit, and
//! gated effect emission. The bytecode VM drives this core from its dispatch
//! loop; generated matchers (the proc-macro backend) drive the same core from
//! compiled per-state code. Keeping the checkpoint contract in one place is
//! the point: a state field added here cannot silently escape save/restore
//! (the exhaustive destructure in [`Engine::restore_checkpoint_state`] will
//! not compile until the field is classified), and both executors inherit the
//! discipline instead of re-implementing it.

use std::num::NonZeroU64;

use tree_sitter::{Node, TreeCursor};

use crate::{
    Checkpoint, CheckpointStack, CheckpointState, CursorWrapper, EffectLog, FrameArena,
    RuntimeEffect,
};

/// Execution state shared by the bytecode VM and generated matchers.
///
/// Instruction pointers, step budgets, and limit policy stay with the
/// executor: they are dispatch concerns, and each executor represents "where
/// am I" differently (decoded step address vs. generated state id).
pub struct Engine<'t> {
    cursor: CursorWrapper<'t>,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    effects: EffectLog<'t>,
    recursion_depth: u32,
    /// Suppression nesting on the active match path: when `> 0`, data effects
    /// are suppressed (not emitted to the log). `SuppressBegin` increments,
    /// `SuppressEnd` decrements. Each open scope lives inside an active call
    /// frame, so it is bounded by call-nesting depth (`recursion_depth`) times
    /// a per-query constant — and call depth is itself capped by the
    /// `u32`-indexed frame arena. A `u16` was far too narrow (deep `@_`
    /// recursion overflowed it at 65_536); `u64` cannot overflow before the
    /// frame arena does.
    suppress_depth: u64,
    /// Whether the cursor's lazy snapshot pool has activated (see
    /// [`CursorWrapper::snapshot`]); once true, every checkpoint push pairs
    /// with a pooled cursor snapshot.
    snapshot_cursor_active: bool,
}

impl<'t> Engine<'t> {
    /// Start an engine at `cursor`'s position (executors pass a root cursor;
    /// see [`CursorWrapper`]'s shared-root invariant).
    pub fn new(cursor: TreeCursor<'t>) -> Self {
        Self::with_initial_suppression(cursor, 0)
    }

    /// Start an engine for a yes/no run: all data effects are suppressed from
    /// the root, so matching can answer without building a committed value.
    /// Query-level suppression scopes still nest above this base depth.
    pub fn new_data_suppressed(cursor: TreeCursor<'t>) -> Self {
        Self::with_initial_suppression(cursor, 1)
    }

    fn with_initial_suppression(cursor: TreeCursor<'t>, suppress_depth: u64) -> Self {
        Self {
            cursor: CursorWrapper::new(cursor),
            frames: FrameArena::new(),
            checkpoints: CheckpointStack::new(),
            effects: EffectLog::new(),
            recursion_depth: 0,
            suppress_depth,
            snapshot_cursor_active: false,
        }
    }

    #[inline]
    pub fn cursor(&self) -> &CursorWrapper<'t> {
        &self.cursor
    }

    #[inline]
    pub fn cursor_mut(&mut self) -> &mut CursorWrapper<'t> {
        &mut self.cursor
    }

    /// The node the cursor is on — the candidate every match check reads.
    #[inline]
    pub fn node(&self) -> Node<'t> {
        self.cursor.node()
    }

    /// The committed effect stream so far.
    #[inline]
    pub fn effects(&self) -> &EffectLog<'t> {
        &self.effects
    }

    /// Surrender the committed effect stream (on Accept).
    #[inline]
    pub fn into_effects(self) -> EffectLog<'t> {
        self.effects
    }

    /// Snapshot the engine state a checkpoint restores on backtrack.
    pub fn checkpoint_state(&self) -> CheckpointState {
        CheckpointState {
            descendant_index: self.cursor.descendant_index(),
            effect_watermark: self.effects.len(),
            frame_index: self.frames.current(),
            recursion_depth: self.recursion_depth,
            suppress_depth: self.suppress_depth,
        }
    }

    /// Restore engine state from a checkpoint's snapshot.
    pub fn restore_checkpoint_state(
        &mut self,
        state: CheckpointState,
        snapshot: Option<NonZeroU64>,
    ) {
        if let Some(snapshot) = snapshot {
            self.cursor.restore(Some(snapshot), state.descendant_index);
        } else if self.cursor.restore_without_snapshot(state.descendant_index) {
            self.snapshot_cursor_active = true;
        }
        self.effects.truncate(state.effect_watermark);
        self.frames.restore(state.frame_index);
        self.recursion_depth = state.recursion_depth;
        self.suppress_depth = state.suppress_depth;
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after checkpoint restore"
        );
        #[cfg(debug_assertions)]
        self.assert_checkpoint_restored(&state);
    }

    /// Assert the post-restore engine state matches the checkpoint snapshot,
    /// and classify every engine field as restored-from or
    /// intentionally-excluded-from [`CheckpointState`]. The exhaustive
    /// destructure is the point: a newly-added engine field will not compile
    /// until it is classified here, so it cannot silently escape the
    /// checkpoint contract. Executor-side state (instruction pointer, step
    /// budget) is resumed separately by the executor's backtrack. Debug-only.
    #[cfg(debug_assertions)]
    fn assert_checkpoint_restored(&self, state: &CheckpointState) {
        let Engine {
            // Restored — must equal the snapshot the checkpoint captured.
            cursor,
            frames,
            effects,
            recursion_depth,
            suppress_depth,
            // Deliberately outside `CheckpointState`:
            checkpoints: _, // the stack this checkpoint was just popped from
            snapshot_cursor_active: _, // cumulative optimization state
        } = self;

        debug_assert_eq!(
            cursor.descendant_index(),
            state.descendant_index,
            "checkpoint restore: cursor position"
        );
        debug_assert_eq!(
            effects.len(),
            state.effect_watermark,
            "checkpoint restore: effect watermark"
        );
        debug_assert_eq!(
            frames.current(),
            state.frame_index,
            "checkpoint restore: frame index"
        );
        debug_assert_eq!(
            *recursion_depth, state.recursion_depth,
            "checkpoint restore: recursion depth"
        );
        debug_assert_eq!(
            *suppress_depth, state.suppress_depth,
            "checkpoint restore: suppress depth"
        );
    }

    /// Push one checkpoint, pairing it with a pooled cursor snapshot once the
    /// snapshot pool has activated.
    pub fn push_checkpoint(&mut self, checkpoint: Checkpoint) {
        if self.snapshot_cursor_active {
            let snapshot = self.cursor_snapshot(1);
            self.checkpoints.push_with_snapshot(checkpoint, snapshot);
        } else {
            self.checkpoints.push(checkpoint);
        }
    }

    /// Push branch checkpoints for every alternative in `alts` — the
    /// non-preferred successors of a branch, in priority order. Pushed in
    /// reverse so LIFO backtracking takes them in order. One state snapshot
    /// serves every push: nothing in the loop moves the cursor or touches the
    /// arenas the snapshot reads.
    pub fn push_branches(&mut self, alts: &[u16]) {
        let state = self.checkpoint_state();
        if self.snapshot_cursor_active {
            let refs = u32::try_from(alts.len()).expect("branch fan-out count fits u32");
            let snapshot = self.cursor_snapshot(refs);
            for &alt in alts.iter().rev() {
                self.checkpoints
                    .push_with_snapshot(Checkpoint::branch(state, alt), snapshot);
            }
        } else {
            for &alt in alts.iter().rev() {
                self.checkpoints.push(Checkpoint::branch(state, alt));
            }
        }
    }

    /// Pop the newest checkpoint (with its pooled snapshot, if any), or `None`
    /// when the stack is exhausted — no match.
    pub fn pop_checkpoint(&mut self) -> Option<(Checkpoint, Option<NonZeroU64>)> {
        if self.snapshot_cursor_active {
            self.checkpoints.pop_with_snapshot()
        } else {
            self.checkpoints.pop().map(|checkpoint| (checkpoint, None))
        }
    }

    fn cursor_snapshot(&mut self, refs: u32) -> NonZeroU64 {
        self.cursor
            .snapshot(refs)
            .expect("snapshot cursor active flag tracks cursor pool activation")
    }

    /// Enter a callee: push a frame returning to `return_addr`.
    pub fn enter_frame(&mut self, return_addr: u16) {
        self.frames.push(return_addr);
        self.recursion_depth += 1;
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after Call"
        );
    }

    /// Exit the current callee, returning its return address. Prunes frames
    /// no live checkpoint can restore (O(1) amortized).
    ///
    /// Panics if no frame is active; executors gate on [`Engine::frames_empty`]
    /// (an empty frame stack at Return means entrypoint acceptance).
    pub fn exit_frame(&mut self) -> u16 {
        let return_addr = self.frames.pop();
        self.recursion_depth = self
            .recursion_depth
            .checked_sub(1)
            .expect("recursion_depth underflow on Return");
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after Return"
        );
        self.frames.prune(self.checkpoints.max_frame_idx());
        return_addr
    }

    #[inline]
    pub fn frames_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Open a suppression scope (bare-ref opacity). Returns whether effects
    /// were already suppressed *before* this open — what a tracer reports.
    pub fn suppress_begin(&mut self) -> bool {
        let was_suppressed = self.suppress_depth > 0;
        self.suppress_depth += 1;
        was_suppressed
    }

    /// Close a suppression scope. Returns whether effects are still
    /// suppressed *after* this close — what a tracer reports.
    pub fn suppress_end(&mut self) -> bool {
        self.suppress_depth = self
            .suppress_depth
            .checked_sub(1)
            .expect("SuppressEnd without matching SuppressBegin");
        self.suppress_depth > 0
    }

    /// Emit a data effect through the suppression gate. The effect is built
    /// lazily so a suppressed `Node` capture never reads the cursor. Returns
    /// the logged effect, or `None` when suppressed.
    #[inline]
    pub fn emit_data(
        &mut self,
        make: impl FnOnce(&CursorWrapper<'t>) -> RuntimeEffect<'t>,
    ) -> Option<&RuntimeEffect<'t>> {
        if self.suppress_depth > 0 {
            return None;
        }
        let effect = make(&self.cursor);
        self.effects.push(effect);
        Some(self.effects.as_slice().last().expect("just pushed"))
    }

    /// Emit an inspection-span effect, bypassing suppression: uncaptured
    /// `(Foo)` bodies still produce source hulls even when they carry no
    /// output bindings.
    #[inline]
    pub fn emit_span(&mut self, effect: RuntimeEffect<'t>) {
        self.effects.push(effect);
    }

    /// Live bytes across the growable runtime arenas — the quantity a memory
    /// ceiling bounds. A sum of element-count × element-size; never allocates.
    pub fn heap_bytes(&self) -> u64 {
        self.frames.byte_footprint()
            + self.checkpoints.byte_footprint()
            + self.effects.byte_footprint()
            + self.cursor.snapshot_footprint()
    }
}
