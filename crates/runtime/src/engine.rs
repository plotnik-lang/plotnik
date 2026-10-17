use std::num::NonZeroU64;

use tree_sitter::Node;

use crate::alloc::Meter;
use crate::tape::{JournalEvent, Tape};
use crate::{
    Checkpoint, CheckpointStack, CheckpointState, CursorWrapper, FrameArena, LimitExceeded,
};

/// Execution state shared by the bytecode VM and generated matchers.
///
/// Instruction pointers stay with the executor, which represents the current
/// instruction differently (decoded code address vs. generated state id).
/// Fuel and memory are charged through the operation's [`Meter`], which the
/// executor passes to every call that can grow storage.
pub struct Engine<'t> {
    cursor: CursorWrapper<'t>,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    tape: Tape<'t>,
    recursion_depth: u32,
    suppress_depth: u32,
    base_suppress_depth: u32,
    scalar_depth: u32,
    snapshot_cursor_active: bool,
}

impl<'t> Engine<'t> {
    /// Start an engine with a cursor rooted at the application node.
    /// See [`CursorWrapper`]'s shared-root invariant.
    pub fn new(cursor: Node<'t>) -> Self {
        Self::with_initial_suppression(cursor, 0)
    }

    /// Start an engine for a yes/no run: all output events are suppressed from
    /// the root, so matching can answer without building a committed value.
    /// Query-level suppression scopes still nest above this base depth.
    pub fn new_match_only(cursor: Node<'t>) -> Self {
        Self::with_initial_suppression(cursor, 1)
    }

    fn with_initial_suppression(cursor: Node<'t>, suppress_depth: u32) -> Self {
        Self {
            cursor: CursorWrapper::new(cursor),
            frames: FrameArena::new(),
            checkpoints: CheckpointStack::new(),
            tape: Tape::new(),
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

    /// The current rollbackable output tape.
    #[inline]
    pub fn tape(&self) -> &Tape<'t> {
        &self.tape
    }

    pub fn take_tape(&mut self) -> Tape<'t> {
        std::mem::take(&mut self.tape)
    }

    /// Abandon the previous candidate, retaining charged capacity for this operation.
    pub fn reset(&mut self, node: Node<'t>) {
        let Self {
            cursor,
            frames,
            checkpoints,
            tape,
            recursion_depth,
            suppress_depth,
            base_suppress_depth,
            scalar_depth,
            snapshot_cursor_active,
        } = self;

        checkpoints.clear();
        frames.clear();
        tape.rollback(Default::default());
        *recursion_depth = 0;
        *suppress_depth = *base_suppress_depth;
        *scalar_depth = 0;
        *snapshot_cursor_active = false;
        cursor.reset(node);
    }

    /// The committed output of an accepted candidate. It stays valid until
    /// the next candidate resets the engine.
    pub fn accepted_tape(&self) -> &Tape<'t> {
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
        &self.tape
    }

    /// Snapshot the engine state a checkpoint restores on backtrack.
    pub fn checkpoint_state(&self) -> CheckpointState {
        CheckpointState {
            descendant_index: self.cursor.descendant_index(),
            tape: self.tape.mark(),
            frame_index: self.frames.current(),
            recursion_depth: self.recursion_depth,
            effect_depths: crate::EffectDepths::new(self.suppress_depth, self.scalar_depth),
        }
    }

    /// Restore engine state from a checkpoint's snapshot.
    pub fn restore_checkpoint_state(
        &mut self,
        meter: &mut Meter<'_>,
        state: CheckpointState,
        snapshot: Option<NonZeroU64>,
    ) -> Result<(), LimitExceeded> {
        if self
            .cursor
            .restore(meter, snapshot, state.descendant_index)?
        {
            self.snapshot_cursor_active = true;
        }
        self.tape.rollback(state.tape);
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
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn assert_checkpoint_restored(&self, state: &CheckpointState) {
        debug_assert_eq!(
            self.cursor.descendant_index(),
            state.descendant_index,
            "checkpoint restore: cursor position"
        );
        debug_assert_eq!(
            self.tape.mark(),
            state.tape,
            "checkpoint restore: tape mark"
        );
        debug_assert_eq!(
            self.frames.current(),
            state.frame_index,
            "checkpoint restore: frame index"
        );
        debug_assert_eq!(
            self.recursion_depth, state.recursion_depth,
            "checkpoint restore: recursion depth"
        );
        debug_assert_eq!(
            self.suppress_depth,
            state.effect_depths.suppression(),
            "checkpoint restore: suppress depth"
        );
        debug_assert_eq!(
            self.scalar_depth,
            state.effect_depths.scalar(),
            "checkpoint restore: scalar depth"
        );
    }

    /// Push one checkpoint, pairing it with a pooled cursor snapshot once the
    /// snapshot pool has activated.
    pub fn push_checkpoint(
        &mut self,
        meter: &mut Meter<'_>,
        checkpoint: Checkpoint,
    ) -> Result<(), LimitExceeded> {
        if !self.snapshot_cursor_active {
            return self.checkpoints.push(meter, checkpoint);
        }
        if !self.checkpoints.reserve(meter, 1, true)? {
            return self.checkpoints.push(meter, checkpoint);
        }

        let snapshot = self.cursor_snapshot(meter, 1)?;
        self.checkpoints
            .push_with_snapshot(meter, checkpoint, snapshot)
    }

    /// Push checkpoints for the non-preferred `successors`, in priority order. Pushed in
    /// reverse so LIFO backtracking takes them in order. One state snapshot
    /// serves every push: nothing in the loop moves the cursor or touches the
    /// arenas the snapshot reads.
    pub fn push_successors<A: Copy + Into<u16>>(
        &mut self,
        meter: &mut Meter<'_>,
        successors: &[A],
    ) -> Result<(), LimitExceeded> {
        let state = self.checkpoint_state();
        let active =
            self.checkpoints
                .reserve(meter, successors.len(), self.snapshot_cursor_active)?;
        if !active {
            for &successor in successors.iter().rev() {
                self.checkpoints
                    .push(meter, Checkpoint::successor(state, successor.into()))?;
            }
            return Ok(());
        }

        let refs = u32::try_from(successors.len()).expect("successor count fits u32");
        let snapshot = self.cursor_snapshot(meter, refs)?;
        for &successor in successors.iter().rev() {
            self.checkpoints.push_with_snapshot(
                meter,
                Checkpoint::successor(state, successor.into()),
                snapshot,
            )?;
        }
        Ok(())
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

    fn cursor_snapshot(
        &mut self,
        meter: &mut Meter<'_>,
        refs: u32,
    ) -> Result<Option<NonZeroU64>, LimitExceeded> {
        self.cursor.snapshot(meter, refs)
    }

    /// Enter a callee, retaining the executor-owned token that identifies the
    /// call's return map.
    pub fn enter_frame(
        &mut self,
        meter: &mut Meter<'_>,
        call_site: u16,
    ) -> Result<(), LimitExceeded> {
        let next_depth =
            self.recursion_depth
                .checked_add(1)
                .ok_or(crate::CallFrameError::RecursionDepth {
                    call_site,
                    current_depth: self.recursion_depth,
                })?;
        self.frames.push(meter, call_site)?;
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

    /// Record an output event through the suppression gate. The event is
    /// built lazily so a suppressed `Node` capture never reads the cursor.
    /// Returns the recorded event, or `None` when suppressed.
    #[inline]
    pub fn emit(
        &mut self,
        meter: &mut Meter<'_>,
        make: impl FnOnce(&CursorWrapper<'t>) -> JournalEvent<'t>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        if self.suppress_depth > 0 {
            return Ok(None);
        }
        let event = make(&self.cursor);
        self.tape.push(meter, event)?;
        Ok(Some(event))
    }

    /// Record an inspection-span event, bypassing suppression: uncaptured
    /// `(Foo)` bodies still produce document bounding ranges even when they carry no
    /// result bindings.
    #[inline]
    pub fn emit_span(
        &mut self,
        meter: &mut Meter<'_>,
        event: JournalEvent<'t>,
    ) -> Result<(), LimitExceeded> {
        self.tape.push(meter, event)
    }

    /// Open a scalar frame through the output-event suppression gate.
    pub fn scalar_open(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        if self.suppress_depth > 0 {
            return Ok(None);
        }
        self.tape.push(meter, JournalEvent::ScalarOpen)?;
        self.scalar_depth = self
            .scalar_depth
            .checked_add(1)
            .expect("scalar frame depth exceeds u32");
        Ok(Some(JournalEvent::ScalarOpen))
    }

    /// Mark the current node when any scalar frame is live. Marks deliberately
    /// cross output-event suppression brackets so an enclosing scalar retains the
    /// provenance of a suppressed nested value.
    pub fn scalar_mark(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        if self.scalar_depth == 0 {
            return Ok(None);
        }
        let event = JournalEvent::ScalarMark(self.node());
        self.tape.push(meter, event)?;
        Ok(Some(event))
    }

    pub fn scalar_close_text(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        self.scalar_close(meter, JournalEvent::TextClose)
    }

    pub fn scalar_close_bool(
        &mut self,
        meter: &mut Meter<'_>,
        value: bool,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        self.scalar_close(meter, JournalEvent::BoolClose(value))
    }

    pub fn node_text(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        self.emit(meter, |cursor| JournalEvent::NodeText(cursor.node()))
    }

    pub fn node_bool(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        self.emit(meter, |cursor| JournalEvent::NodeBool(cursor.node()))
    }

    pub fn bool_value(
        &mut self,
        meter: &mut Meter<'_>,
        value: bool,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        self.emit(meter, |_| JournalEvent::BoolValue(value))
    }

    fn scalar_close(
        &mut self,
        meter: &mut Meter<'_>,
        event: JournalEvent<'t>,
    ) -> Result<Option<JournalEvent<'t>>, LimitExceeded> {
        if self.suppress_depth > 0 {
            return Ok(None);
        }
        self.tape.push(meter, event)?;
        self.scalar_depth = self
            .scalar_depth
            .checked_sub(1)
            .expect("scalar close without an open scalar frame");
        Ok(Some(event))
    }
}
