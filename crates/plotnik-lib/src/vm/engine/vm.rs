//! Virtual machine for executing compiled Plotnik queries.

use arborium_tree_sitter::Tree;

use crate::bytecode::{
    Call, Effect, EffectKind, Entrypoint, Instruction, Match, Module, Nav, NodeKindConstraint,
    PredicateOp,
};

use crate::core::NodeFieldId;

use super::checkpoint::{CallResume, Checkpoint, CheckpointStack, CheckpointState};
use super::cursor::{CursorWrapper, SkipPolicy};
use super::effect::{EffectLog, RuntimeEffect};
use super::error::{ControlFlow, RuntimeError, Signal};
use super::frame::FrameArena;
use super::limits::{ResolvedRuntimeLimits, RuntimeLimitSpec};
use super::trace::{NoopTracer, Tracer};

/// Virtual machine state for query execution.
pub struct VM<'t> {
    pub(crate) cursor: CursorWrapper<'t>,
    /// Current instruction pointer (raw u16, 0 is valid at runtime).
    pub(crate) ip: u16,
    pub(crate) frames: FrameArena,
    pub(crate) checkpoints: CheckpointStack,
    pub(crate) effects: EffectLog<'t>,

    pub(crate) steps_used: u64,
    pub(crate) recursion_depth: u32,
    pub(crate) limits: ResolvedRuntimeLimits,

    /// Suppression nesting on the active match path: when `> 0`, effects are
    /// suppressed (not emitted to the log). `SuppressBegin` increments,
    /// `SuppressEnd` decrements. Each open scope lives inside an active call frame,
    /// so it is bounded by call-nesting depth (`recursion_depth`) times a per-query
    /// constant — and call depth is itself capped by the `u32`-indexed frame arena.
    /// A `u16` was far too narrow (deep `@_` recursion overflowed it at 65_536);
    /// `u64` cannot overflow before the frame arena does.
    pub(crate) suppress_depth: u64,

    pub(crate) source: &'t str,
}

/// Builder for VM instances.
pub struct VMBuilder<'t> {
    source: &'t str,
    tree: &'t Tree,
    spec: RuntimeLimitSpec,
}

impl<'t> VMBuilder<'t> {
    pub fn new(source: &'t str, tree: &'t Tree) -> Self {
        Self {
            source,
            tree,
            spec: RuntimeLimitSpec::default(),
        }
    }

    /// Set the runtime limit policy. `Auto` limits are sized from the source
    /// tree's node count when [`Self::build`] resolves them.
    pub fn limits(mut self, spec: RuntimeLimitSpec) -> Self {
        self.spec = spec;
        self
    }

    /// Build the VM, resolving `Auto` limits against the source's node count.
    pub fn build(self) -> VM<'t> {
        let source_nodes =
            u32::try_from(self.tree.root_node().descendant_count()).unwrap_or(u32::MAX);
        VM {
            cursor: CursorWrapper::new(self.tree.walk()),
            ip: 0,
            frames: FrameArena::new(),
            checkpoints: CheckpointStack::new(),
            effects: EffectLog::new(),
            steps_used: 0,
            recursion_depth: 0,
            limits: self.spec.resolve(source_nodes),
            suppress_depth: 0,
            source: self.source,
        }
    }
}

impl<'t> VM<'t> {
    pub fn builder(source: &'t str, tree: &'t Tree) -> VMBuilder<'t> {
        VMBuilder::new(source, tree)
    }

    /// Snapshot the VM state a checkpoint restores on backtrack.
    fn checkpoint_state(&self) -> CheckpointState {
        CheckpointState {
            descendant_index: self.cursor.descendant_index(),
            effect_watermark: self.effects.len(),
            frame_index: self.frames.current(),
            recursion_depth: self.recursion_depth,
            suppress_depth: self.suppress_depth,
        }
    }

    /// Restore VM state from a checkpoint's snapshot.
    fn restore_checkpoint_state(&mut self, state: CheckpointState) {
        self.cursor.goto_descendant(state.descendant_index);
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

    /// Assert the post-restore VM state matches the checkpoint snapshot, and
    /// classify every VM field as restored-from or intentionally-excluded-from
    /// `CheckpointState`. The exhaustive destructure is the point: a newly-added
    /// VM field will not compile until it is classified here, so it cannot
    /// silently escape the checkpoint contract. `ip` is resumed separately by
    /// [`Self::backtrack`]. Debug-only.
    #[cfg(debug_assertions)]
    fn assert_checkpoint_restored(&self, state: &CheckpointState) {
        let VM {
            // Restored — must equal the snapshot the checkpoint captured.
            cursor,
            frames,
            effects,
            recursion_depth,
            suppress_depth,
            // Deliberately outside `CheckpointState`:
            ip: _,          // resumed separately by `backtrack` (cp.ip / call_resume)
            checkpoints: _, // the stack this checkpoint was just popped from
            steps_used: _,  // monotonic step counter, never rewound on backtrack
            limits: _,      // immutable execution config
            source: _,      // immutable input text
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

    /// Checkpoint that resumes a branch alternative at `ip`.
    fn branch_checkpoint(&self, ip: u16) -> Checkpoint {
        Checkpoint::branch(self.checkpoint_state(), ip)
    }

    /// Checkpoint that, on backtrack, advances the cursor and re-enters the
    /// callee. `call_ip` is the Call's address (for trace rendering only).
    fn call_retry_checkpoint(&self, call_ip: u16, resume: CallResume) -> Checkpoint {
        Checkpoint::call_retry(self.checkpoint_state(), call_ip, resume)
    }

    /// Live bytes across the three growable runtime arenas (frame, checkpoint,
    /// and effect heaps) — the quantity the memory ceiling bounds. A sum of
    /// element-count × element-size; it never allocates.
    fn heap_bytes(&self) -> u64 {
        self.frames.byte_footprint()
            + self.checkpoints.byte_footprint()
            + self.effects.byte_footprint()
    }

    /// Execute query from entrypoint, returning effect log.
    ///
    /// This is a convenience method that uses `NoopTracer`, which gets
    /// completely optimized away at compile time.
    pub fn execute(
        self,
        module: &Module,
        entrypoint: &Entrypoint,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        self.execute_with(module, entrypoint, &mut NoopTracer)
    }

    /// Execute query with a tracer for debugging.
    ///
    /// The tracer is generic, so `NoopTracer` calls are optimized away
    /// while `PrintTracer` calls collect execution trace.
    ///
    pub fn execute_with<T: Tracer>(
        mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        self.ip = u16::from(entrypoint.target());
        if T::ENABLED {
            tracer.trace_enter_entrypoint(self.ip);
        }

        loop {
            // Step ceiling: bound total work. `None` opts out (Unbounded).
            if let Some(max) = self.limits.max_steps
                && self.steps_used >= max
            {
                return Err(RuntimeError::StepLimitExceeded(max));
            }
            self.steps_used += 1;

            // Memory ceiling: bound the live runtime heap, sampled once per
            // dispatch. Per-step growth is bounded, so this catches blowup
            // promptly. `None` opts out (Unbounded).
            if let Some(max) = self.limits.max_memory {
                let used = self.heap_bytes();
                if used > max {
                    return Err(RuntimeError::MemoryLimitExceeded { used, limit: max });
                }
            }

            // Fetch and dispatch. The IP must address a validated instruction
            // start; a violation localizes a bad jump to the step that wrote `ip`,
            // before `decode_step` begins decoding mid-instruction.
            #[cfg(debug_assertions)]
            debug_assert!(
                module.is_validated_step_start(self.ip),
                "ip {} is not a validated instruction start",
                self.ip
            );
            let instr = module.decode_step(self.ip);
            if T::ENABLED {
                tracer.trace_instruction(self.ip, &instr);
            }

            let result = match instr {
                Instruction::Match(m) => self.exec_match(m, module, tracer),
                Instruction::Call(c) => self.exec_call(c, tracer),
                Instruction::Return(_) => self.exec_return(tracer),
            };

            match result {
                Ok(()) | Err(Signal::Flow(ControlFlow::Backtracked)) => continue,
                Err(Signal::Flow(ControlFlow::Accept)) => return Ok(self.effects),
                Err(Signal::Error(e)) => return Err(e),
            }
        }
    }

    fn exec_match<T: Tracer>(
        &mut self,
        m: Match<'_>,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if !m.is_epsilon() {
            self.navigate_and_match(m, module, tracer)?;
        }

        for effect_op in m.effects() {
            self.emit_effect(effect_op, tracer);
        }

        self.branch_to_successors(m, tracer)
    }

    fn navigate_and_match<T: Tracer>(
        &mut self,
        m: Match<'_>,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let Some(policy) = self.cursor.navigate(m.nav) else {
            if T::ENABLED {
                tracer.trace_nav_failure(m.nav);
            }
            return Err(self.backtrack(tracer));
        };
        if T::ENABLED {
            tracer.trace_nav(m.nav, self.cursor.node());
        }

        let cont_nav = m.nav.sibling_continuation();
        loop {
            if self.candidate_matches(m, module, tracer) {
                break;
            }
            self.advance_or_backtrack(policy, cont_nav, tracer)?;
        }

        if T::ENABLED {
            tracer.trace_match_success(self.cursor.node());
        }
        if T::ENABLED
            && let Some(field_id) = m.node_field
        {
            tracer.trace_field_success(field_id);
        }

        Ok(())
    }

    /// `op` selects the operator (see [`PredicateOp`]); `is_regex` chooses
    /// RegexTable over StringTable for `value_ref`.
    fn evaluate_predicate(&self, op: u8, is_regex: bool, value_ref: u16, module: &Module) -> bool {
        let node = self.cursor.node();
        let node_text = node
            .utf8_text(self.source.as_bytes())
            .expect("node source text must be valid UTF-8");
        let op = PredicateOp::from_byte(op);

        if is_regex {
            // The DFAs are deserialized once at `Module::load` and reused here;
            // `RegexDfas::is_match` upholds the populated-slot invariant that a
            // module passing load guarantees. Deserializing per evaluation, as
            // this once did, re-validated the whole automaton on every predicate
            // test (issue #426).
            let matched = module.regex_dfas().is_match(value_ref as usize, node_text);

            match op {
                PredicateOp::RegexMatch => matched,
                PredicateOp::RegexNoMatch => !matched,
                _ => unreachable!("non-regex op with is_regex=true"),
            }
        } else {
            let target = module.strings().at(value_ref as usize);

            match op {
                PredicateOp::Eq => node_text == target,
                PredicateOp::Ne => node_text != target,
                PredicateOp::StartsWith => node_text.starts_with(target),
                PredicateOp::EndsWith => node_text.ends_with(target),
                PredicateOp::Contains => node_text.contains(target),
                _ => unreachable!("regex op with is_regex=false"),
            }
        }
    }

    fn candidate_matches<T: Tracer>(&self, m: Match<'_>, module: &Module, tracer: &mut T) -> bool {
        let node = self.cursor.node();

        match m.node_kind {
            NodeKindConstraint::Any => {}
            NodeKindConstraint::Named(None) => {
                if !node.is_named() {
                    if T::ENABLED {
                        tracer.trace_match_failure(node);
                    }
                    return false;
                }
            }
            NodeKindConstraint::Named(Some(expected)) => {
                if !node.is_named() || node.kind_id() != u16::from(expected) {
                    if T::ENABLED {
                        tracer.trace_match_failure(node);
                    }
                    return false;
                }
            }
            NodeKindConstraint::Anonymous(None) => {
                if node.is_named() {
                    if T::ENABLED {
                        tracer.trace_match_failure(node);
                    }
                    return false;
                }
            }
            NodeKindConstraint::Anonymous(Some(expected)) => {
                if node.is_named() || node.kind_id() != u16::from(expected) {
                    if T::ENABLED {
                        tracer.trace_match_failure(node);
                    }
                    return false;
                }
            }
        }

        if let Some(expected) = m.node_field
            && self.cursor.field_id() != Some(expected)
        {
            if T::ENABLED {
                tracer.trace_field_failure(node);
            }
            return false;
        }

        for field_id in m.neg_fields() {
            if node.child_by_field_id(u16::from(field_id)).is_some() {
                return false;
            }
        }

        if let Some(predicate) = m.predicate()
            && !self.evaluate_predicate(
                predicate.op,
                predicate.is_regex,
                predicate.value_ref,
                module,
            )
        {
            return false;
        }

        true
    }

    fn branch_to_successors<T: Tracer>(
        &mut self,
        m: Match<'_>,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if m.succ_count() == 0 {
            return Err(ControlFlow::Accept.into());
        }

        // Push checkpoints for alternate branches (in reverse order)
        for i in (1..m.succ_count()).rev() {
            self.checkpoints
                .push(self.branch_checkpoint(u16::from(m.successor(i))));
            if T::ENABLED {
                tracer.trace_checkpoint_created(self.ip);
            }
        }

        self.ip = u16::from(m.successor(0));
        Ok(())
    }

    fn exec_call<T: Tracer>(&mut self, c: Call, tracer: &mut T) -> Result<(), Signal> {
        let skip_policy = self.navigate_to_field_with_policy(c.nav, c.node_field, tracer)?;

        // A searchable nav leaves a retry checkpoint so the callee can be
        // re-tried at later siblings if it fails. Exact/Stay navs have a fixed
        // candidate and need no retry.
        if let Some(policy) = skip_policy
            && policy != SkipPolicy::Exact
        {
            let resume = CallResume {
                target: u16::from(c.target),
                next: u16::from(c.next),
                field: c.node_field,
                policy,
            };
            self.checkpoints
                .push(self.call_retry_checkpoint(self.ip, resume));
            if T::ENABLED {
                tracer.trace_checkpoint_created(self.ip);
            }
        }

        self.enter_callee(u16::from(c.target), u16::from(c.next), tracer);
        Ok(())
    }

    /// Push a frame for `target` (returning to `next`) and jump in.
    fn enter_callee<T: Tracer>(&mut self, target: u16, next: u16, tracer: &mut T) {
        if T::ENABLED {
            tracer.trace_call(target);
        }
        self.frames.push(next);
        self.recursion_depth += 1;
        debug_assert_eq!(
            self.recursion_depth,
            self.frames.depth(),
            "recursion_depth desynced from frame stack after Call"
        );
        self.ip = target;
    }

    /// Navigate to a field and return the skip policy for retry support.
    ///
    /// Returns `Some(policy)` if navigation was performed, `None` if Stay nav was used.
    fn navigate_to_field_with_policy<T: Tracer>(
        &mut self,
        nav: Nav,
        field: Option<NodeFieldId>,
        tracer: &mut T,
    ) -> Result<Option<SkipPolicy>, Signal> {
        if nav == Nav::Stay || nav == Nav::StayExact {
            self.check_field(field, tracer)?;
            return Ok(None);
        }

        let Some(policy) = self.cursor.navigate(nav) else {
            if T::ENABLED {
                tracer.trace_nav_failure(nav);
            }
            return Err(self.backtrack(tracer));
        };
        if T::ENABLED {
            tracer.trace_nav(nav, self.cursor.node());
        }

        let Some(field_id) = field else {
            return Ok(Some(policy));
        };

        let cont_nav = nav.sibling_continuation();
        loop {
            if self.cursor.field_id() == Some(field_id) {
                if T::ENABLED {
                    tracer.trace_field_success(field_id);
                }
                return Ok(Some(policy));
            }
            if T::ENABLED {
                tracer.trace_field_failure(self.cursor.node());
            }
            self.advance_or_backtrack(policy, cont_nav, tracer)?;
        }
    }

    fn check_field<T: Tracer>(
        &mut self,
        field: Option<NodeFieldId>,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let Some(field_id) = field else {
            return Ok(());
        };
        if self.cursor.field_id() != Some(field_id) {
            if T::ENABLED {
                tracer.trace_field_failure(self.cursor.node());
            }
            return Err(self.backtrack(tracer));
        }
        if T::ENABLED {
            tracer.trace_field_success(field_id);
        }
        Ok(())
    }

    fn exec_return<T: Tracer>(&mut self, tracer: &mut T) -> Result<(), Signal> {
        if T::ENABLED {
            tracer.trace_return();
        }

        // If no frames, we're returning from top-level entrypoint → Accept
        if self.frames.is_empty() {
            return Err(ControlFlow::Accept.into());
        }

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

        // Prune frames (O(1) amortized)
        self.frames.prune(self.checkpoints.max_frame_idx());

        self.ip = return_addr;
        Ok(())
    }

    // Loops rather than self-recurses: a run of contiguous call-retry checkpoints
    // with exhausted siblings (or failed field constraints) is unwound here in one
    // call. The depth of that run is set by the source-tree shape and is decoupled
    // from call depth, so tail-recursion would let untrusted source abort the
    // process on the native stack (Rust does not guarantee TCO). The `continue`
    // paths pop without re-pushing, so the checkpoint stack strictly shrinks until
    // a resume succeeds or it empties — the loop always terminates.
    fn backtrack<T: Tracer>(&mut self, tracer: &mut T) -> Signal {
        loop {
            let Some(cp) = self.checkpoints.pop() else {
                return RuntimeError::NoMatch.into();
            };
            if T::ENABLED {
                tracer.trace_backtrack();
            }
            self.restore_checkpoint_state(cp.state);

            let Some(resume) = cp.call_resume else {
                self.ip = cp.ip;
                return ControlFlow::Backtracked.into();
            };

            // Call retry: advance to the next candidate, then re-enter the callee.
            // If siblings are exhausted, keep backtracking to an earlier checkpoint.
            if !self.cursor.continue_search(resume.policy) {
                continue;
            }
            if T::ENABLED {
                tracer.trace_nav(Nav::Down.sibling_continuation(), self.cursor.node());
            }

            // Enforce the field constraint at the new candidate. A mismatch ends
            // this Call's search, exactly like the navigate-time field check.
            if let Some(field_id) = resume.field {
                if self.cursor.field_id() != Some(field_id) {
                    if T::ENABLED {
                        tracer.trace_field_failure(self.cursor.node());
                    }
                    continue;
                }
                if T::ENABLED {
                    tracer.trace_field_success(field_id);
                }
            }

            self.checkpoints
                .push(self.call_retry_checkpoint(cp.ip, resume));
            if T::ENABLED {
                tracer.trace_checkpoint_created(cp.ip);
            }
            self.enter_callee(resume.target, resume.next, tracer);
            return ControlFlow::Backtracked.into();
        }
    }

    fn advance_or_backtrack<T: Tracer>(
        &mut self,
        policy: SkipPolicy,
        cont_nav: Nav,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if !self.cursor.continue_search(policy) {
            return Err(self.backtrack(tracer));
        }
        if T::ENABLED {
            tracer.trace_nav(cont_nav, self.cursor.node());
        }
        Ok(())
    }

    fn emit_effect<T: Tracer>(&mut self, op: Effect, tracer: &mut T) {
        use EffectKind::*;

        let effect = match op.kind {
            SuppressBegin => {
                if T::ENABLED {
                    tracer.trace_suppress_control(SuppressBegin, self.suppress_depth > 0);
                }
                self.suppress_depth += 1;
                return;
            }
            SuppressEnd => {
                self.suppress_depth = self
                    .suppress_depth
                    .checked_sub(1)
                    .expect("SuppressEnd without matching SuppressBegin");
                if T::ENABLED {
                    tracer.trace_suppress_control(SuppressEnd, self.suppress_depth > 0);
                }
                return;
            }

            // Skip data effects when suppressing, but trace them
            _ if self.suppress_depth > 0 => {
                if T::ENABLED {
                    tracer.trace_effect_suppressed(op.kind, op.payload);
                }
                return;
            }

            Node => RuntimeEffect::Node(self.cursor.node()),
            ArrayOpen => RuntimeEffect::ArrayOpen,
            Push => RuntimeEffect::Push,
            ArrayClose => RuntimeEffect::ArrayClose,
            StructOpen => RuntimeEffect::StructOpen,
            StructClose => RuntimeEffect::StructClose,
            Set => RuntimeEffect::Set(op.payload as u16),
            EnumOpen => RuntimeEffect::EnumOpen(op.payload as u16),
            EnumClose => RuntimeEffect::EnumClose,
            Null => RuntimeEffect::Null,
        };

        if T::ENABLED {
            tracer.trace_effect(&effect);
        }
        self.effects.push(effect);
    }
}
