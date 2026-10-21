use std::num::NonZeroU64;

use tree_sitter::{Node, TreeCursor};

use crate::{
    CallFrameError, Checkpoint, CheckpointStack, CheckpointState, CursorWrapper, FrameArena,
    JournalEvent, MatchJournal,
};

/// Execution state shared by the bytecode VM and generated matchers.
///
/// Instruction pointers stay with the executor, which represents the current
/// instruction differently (decoded code address vs. generated state id).
pub struct Engine<'t> {
    cursor: CursorWrapper<'t>,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    journal: MatchJournal<'t>,
    recursion_depth: u32,
    suppress_depth: u32,
    base_suppress_depth: u32,
    scalar_depth: u32,
    snapshot_cursor_active: bool,
}

impl<'t> Engine<'t> {
    /// Start an engine with a cursor rooted at the application node.
    /// See [`CursorWrapper`]'s shared-root invariant.
    pub fn new(cursor: TreeCursor<'t>) -> Self {
        Self::with_initial_suppression(cursor, 0)
    }

    /// Start an engine for a yes/no run: all output events are suppressed from
    /// the root, so matching can answer without building a committed value.
    /// Query-level suppression scopes still nest above this base depth.
    pub fn new_match_only(cursor: TreeCursor<'t>) -> Self {
        Self::with_initial_suppression(cursor, 1)
    }

    fn with_initial_suppression(cursor: TreeCursor<'t>, suppress_depth: u32) -> Self {
        Self {
            cursor: CursorWrapper::new(cursor),
            frames: FrameArena::new(),
            checkpoints: CheckpointStack::new(),
            journal: MatchJournal::new(),
            recursion_depth: 0,
            suppress_depth,
            base_suppress_depth: suppress_depth,
            scalar_depth: 0,
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

    /// The current rollbackable match journal.
    #[inline]
    pub fn journal(&self) -> &MatchJournal<'t> {
        &self.journal
    }

    /// Surrender the committed match journal after acceptance.
    #[inline]
    pub fn into_journal(self) -> MatchJournal<'t> {
        self.accepted_journal();
        self.journal
    }

    /// Abandon the previous candidate, retaining scratch capacity for this operation.
    pub fn reset(&mut self, node: Node<'t>) {
        let Self {
            cursor,
            frames,
            checkpoints,
            journal,
            recursion_depth,
            suppress_depth,
            base_suppress_depth,
            scalar_depth,
            snapshot_cursor_active,
        } = self;

        checkpoints.clear();
        frames.clear();
        journal.truncate(0);
        *recursion_depth = 0;
        *suppress_depth = *base_suppress_depth;
        *scalar_depth = 0;
        *snapshot_cursor_active = false;
        cursor.reset(node);
    }

    /// Borrow accepted output only until the next candidate resets scratch.
    /// Decoders must return values independent of this journal.
    pub fn accepted_journal(&self) -> &MatchJournal<'t> {
        assert!(
            self.frames.is_empty(),
            "runtime accepted a match with a live call frame: current_frame={:?}",
            self.frames.current()
        );
        assert_eq!(
            self.recursion_depth, 0,
            "runtime accepted a match with nonzero recursion depth"
        );
        assert_eq!(
            self.suppress_depth, self.base_suppress_depth,
            "runtime accepted a match with unbalanced suppression effects"
        );
        assert_eq!(
            self.scalar_depth, 0,
            "runtime accepted a match with unclosed scalar output effects"
        );
        &self.journal
    }

    /// Snapshot the engine state a checkpoint restores on backtrack.
    pub fn checkpoint_state(&self) -> CheckpointState {
        CheckpointState {
            descendant_index: self.cursor.descendant_index(),
            journal_watermark: self.journal.len(),
            frame_index: self.frames.current(),
            recursion_depth: self.recursion_depth,
            effect_depths: crate::EffectDepths::new(self.suppress_depth, self.scalar_depth),
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
        self.journal.truncate(state.journal_watermark);
        self.frames.restore(state.frame_index);
        self.recursion_depth = state.recursion_depth;
        self.suppress_depth = state.effect_depths.suppression();
        self.scalar_depth = state.effect_depths.scalar();
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after checkpoint restore"
        );
        #[cfg(debug_assertions)]
        self.assert_checkpoint_restored(&state);
    }

    #[cfg(debug_assertions)]
    fn assert_checkpoint_restored(&self, state: &CheckpointState) {
        let Engine {
            cursor,
            frames,
            journal,
            recursion_depth,
            suppress_depth,
            base_suppress_depth: _,
            scalar_depth,
            checkpoints: _,
            snapshot_cursor_active: _,
        } = self;

        debug_assert_eq!(
            cursor.descendant_index(),
            state.descendant_index,
            "checkpoint restore: cursor position"
        );
        debug_assert_eq!(
            journal.len(),
            state.journal_watermark,
            "checkpoint restore: journal watermark"
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
            *suppress_depth,
            state.effect_depths.suppression(),
            "checkpoint restore: suppress depth"
        );
        debug_assert_eq!(
            *scalar_depth,
            state.effect_depths.scalar(),
            "checkpoint restore: scalar depth"
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

    /// Push checkpoints for the non-preferred `successors`, in priority order. Pushed in
    /// reverse so LIFO backtracking takes them in order. One state snapshot
    /// serves every push: nothing in the loop moves the cursor or touches the
    /// arenas the snapshot reads.
    pub fn push_successors<A: Copy + Into<u16>>(&mut self, successors: &[A]) {
        let state = self.checkpoint_state();
        if self.snapshot_cursor_active {
            let refs = u32::try_from(successors.len()).expect("successor count fits u32");
            let snapshot = self.cursor_snapshot(refs);
            for &successor in successors.iter().rev() {
                self.checkpoints
                    .push_with_snapshot(Checkpoint::successor(state, successor.into()), snapshot);
            }
        } else {
            for &successor in successors.iter().rev() {
                self.checkpoints
                    .push(Checkpoint::successor(state, successor.into()));
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

    /// Enter a callee, retaining the executor-owned token that identifies the
    /// call's return map.
    pub fn enter_frame(&mut self, call_site: u16) -> Result<(), CallFrameError> {
        let next_depth =
            self.recursion_depth
                .checked_add(1)
                .ok_or(CallFrameError::RecursionDepth {
                    call_site,
                    current_depth: self.recursion_depth,
                })?;
        self.frames.push(call_site)?;
        self.recursion_depth = next_depth;
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after Call"
        );
        Ok(())
    }

    /// Exit the current callee, returning its call-site token. The executor
    /// combines this token with the callee's [`crate::PortId`] to select a
    /// continuation. Prunes frames no live checkpoint can restore (O(1)
    /// amortized).
    ///
    /// Panics if no frame is active; executors gate on [`Engine::frames_empty`]
    /// (an empty frame stack at Return means entrypoint acceptance).
    pub fn exit_frame(&mut self) -> u16 {
        let call_site = self.frames.pop();
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
        call_site
    }

    #[inline]
    pub fn frames_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Open a suppression scope (bare-ref opacity). Returns whether output events
    /// were already suppressed *before* this open — what a tracer reports.
    pub fn suppress_begin(&mut self) -> bool {
        let was_suppressed = self.suppress_depth > 0;
        self.suppress_depth = self
            .suppress_depth
            .checked_add(1)
            .expect("suppression depth exceeds u32");
        was_suppressed
    }

    /// Close a suppression scope. Returns whether output events are still
    /// suppressed *after* this close — what a tracer reports.
    pub fn suppress_end(&mut self) -> bool {
        self.suppress_depth = self
            .suppress_depth
            .checked_sub(1)
            .expect("SuppressEnd without matching SuppressBegin");
        self.suppress_depth > 0
    }

    /// Emit an output event through the suppression gate. The event is built
    /// lazily so a suppressed `Node` capture never reads the cursor. Returns
    /// the journaled event, or `None` when suppressed.
    #[inline]
    pub fn emit_output_event(
        &mut self,
        make: impl FnOnce(&CursorWrapper<'t>) -> JournalEvent<'t>,
    ) -> Option<&JournalEvent<'t>> {
        if self.suppress_depth > 0 {
            return None;
        }
        let event = make(&self.cursor);
        self.journal.push(event);
        Some(self.journal.as_slice().last().expect("just pushed"))
    }

    /// Emit an inspection-span event, bypassing suppression: uncaptured
    /// `(Foo)` bodies still produce document bounding ranges even when they carry no
    /// result bindings.
    #[inline]
    pub fn emit_span(&mut self, event: JournalEvent<'t>) {
        self.journal.push(event);
    }

    /// Open a scalar frame through the output-event suppression gate.
    pub fn scalar_open(&mut self) -> Option<&JournalEvent<'t>> {
        if self.suppress_depth > 0 {
            return None;
        }
        self.scalar_depth = self
            .scalar_depth
            .checked_add(1)
            .expect("scalar frame depth exceeds u32");
        self.journal.push(JournalEvent::ScalarOpen);
        Some(self.journal.as_slice().last().expect("just pushed"))
    }

    /// Mark the current node when any scalar frame is live. Marks deliberately
    /// cross output-event suppression brackets so an enclosing scalar retains the
    /// provenance of a suppressed nested value.
    pub fn scalar_mark(&mut self) -> Option<&JournalEvent<'t>> {
        if self.scalar_depth == 0 {
            return None;
        }
        self.journal.push(JournalEvent::ScalarMark(self.node()));
        Some(self.journal.as_slice().last().expect("just pushed"))
    }

    pub fn scalar_close_text(&mut self) -> Option<&JournalEvent<'t>> {
        self.scalar_close(JournalEvent::TextClose)
    }

    pub fn scalar_close_bool(&mut self, value: bool) -> Option<&JournalEvent<'t>> {
        self.scalar_close(JournalEvent::BoolClose(value))
    }

    pub fn node_text(&mut self) -> Option<&JournalEvent<'t>> {
        self.emit_output_event(|cursor| JournalEvent::NodeText(cursor.node()))
    }

    pub fn node_bool(&mut self) -> Option<&JournalEvent<'t>> {
        self.emit_output_event(|cursor| JournalEvent::NodeBool(cursor.node()))
    }

    pub fn bool_value(&mut self, value: bool) -> Option<&JournalEvent<'t>> {
        self.emit_output_event(|_| JournalEvent::BoolValue(value))
    }

    fn scalar_close(&mut self, event: JournalEvent<'t>) -> Option<&JournalEvent<'t>> {
        if self.suppress_depth > 0 {
            return None;
        }
        self.scalar_depth = self
            .scalar_depth
            .checked_sub(1)
            .expect("scalar close without an open scalar frame");
        self.journal.push(event);
        Some(self.journal.as_slice().last().expect("just pushed"))
    }

    /// Matcher-heap estimate for runtime statistics. Arena elements
    /// use length, excluding spare capacity retained across candidate resets.
    /// Pooled cursor estimates stay charged even when recycled. Never allocates.
    pub fn heap_bytes(&self) -> u64 {
        self.frames.byte_footprint()
            + self.checkpoints.byte_footprint()
            + self.journal.byte_footprint()
            + self.cursor.snapshot_footprint()
    }
}
