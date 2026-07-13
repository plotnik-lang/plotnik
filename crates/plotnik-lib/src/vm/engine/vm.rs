//! Virtual machine for executing compiled Plotnik queries.

use tree_sitter::Tree;

use crate::bytecode::{
    CodeAddr, DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, DecodedRoutedCall,
    DecodedSplitCall, Effect, EffectKind, Entrypoint, Module, Nav, NodeKindConstraint, PredicateOp,
    SuccessorAddr,
};

use crate::core::NodeFieldId;

use plotnik_rt::{
    CallResume, Checkpoint, Engine, JournalEvent, MatchJournal, ResolvedRuntimeLimits, Resume,
    ReturnOutcome, RuntimeLimitSpec, SkipPolicy,
};

use super::error::{ControlFlow, RuntimeError, Signal};
use super::trace::{NoopTracer, Tracer};
use super::value::node_text;

/// Bitmask selecting the matcher dispatches on which the memory ceiling is
/// sampled; must be a power of two minus one.
const MEMORY_SAMPLE_MASK: u64 = 1024 - 1;

/// Resource usage observed during one VM run.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RunStats {
    /// Fuel consumed by matcher execution. Each matcher dispatch currently
    /// consumes one unit; this is not a stable cross-version performance metric.
    pub fuel_used: u64,
    /// Peak live runtime heap observed at memory-sampling points and run exit.
    pub heap_high_water: u64,
}

/// Virtual machine state for query execution.
///
/// The engine core — cursor, frames, checkpoints, match journal, suppression —
/// lives in [`plotnik_rt::Engine`], shared with generated matchers so the
/// checkpoint contract stays single-sourced. The VM keeps only the
/// interpretive layer: the instruction pointer into decoded bytecode and the
/// fuel/memory budget.
pub struct VM<'t> {
    pub(crate) engine: Engine<'t>,
    /// Current address in the decoded instruction stream.
    pub(crate) ip: CodeAddr,

    pub(crate) fuel_used: u64,
    pub(crate) limits: ResolvedRuntimeLimits,

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
            engine: Engine::new(self.tree.walk()),
            ip: CodeAddr::ZERO,
            fuel_used: 0,
            limits: self.spec.resolve(source_nodes),
            source: self.source,
        }
    }
}

impl<'t> VM<'t> {
    pub fn builder(source: &'t str, tree: &'t Tree) -> VMBuilder<'t> {
        VMBuilder::new(source, tree)
    }

    /// Checkpoint that, on backtrack, advances the cursor and re-enters the
    /// callee. `call_ip` is the Call's address (for trace rendering only).
    fn call_retry_checkpoint(&self, call_ip: CodeAddr, resume: CallResume) -> Checkpoint {
        Checkpoint::call_retry(self.engine.checkpoint_state(), u16::from(call_ip), resume)
    }

    /// Execute a query from an entrypoint, returning its committed match journal.
    ///
    /// This is a convenience method that uses `NoopTracer`, which gets
    /// completely optimized away at compile time.
    pub fn execute(
        self,
        module: &Module,
        entrypoint: &Entrypoint,
    ) -> Result<MatchJournal<'t>, RuntimeError> {
        self.execute_with(module, entrypoint, &mut NoopTracer)
    }

    /// Execute query with a tracer for debugging.
    ///
    /// The tracer is generic, so `NoopTracer` calls are optimized away
    /// while `PrintTracer` calls collect execution trace.
    ///
    pub fn execute_with<T: Tracer>(
        self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<MatchJournal<'t>, RuntimeError> {
        let (result, _) = self.execute_with_stats(module, entrypoint, tracer);
        result
    }

    /// Execute query with a tracer and report run statistics.
    pub fn execute_with_stats<T: Tracer>(
        mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> (Result<MatchJournal<'t>, RuntimeError>, RunStats) {
        self.ip = entrypoint.target();
        if T::ENABLED {
            tracer.trace_enter_entrypoint(self.ip);
        }

        let mut heap_high_water = self.engine.heap_bytes();

        loop {
            // One matcher dispatch currently consumes one fuel unit.
            if let Some(limit) = self.limits.fuel_limit
                && self.fuel_used >= limit
            {
                let stats = self.finish_stats(&mut heap_high_water);
                return (Err(RuntimeError::OutOfFuel(limit)), stats);
            }
            self.fuel_used += 1;

            // Memory ceiling: bound the live runtime heap, sampled every
            // `MEMORY_SAMPLE_MASK + 1` dispatches. Per-dispatch growth is bounded
            // (≤30 checkpoints + ≤15 effects + 1 frame + ≤1 pooled snapshot
            // ≈ 4.4 KiB), so sampling every 1024 dispatches bounds the unobserved
            // overshoot to ~4.5 MiB — noise against the ≥64 MiB auto ceiling.
            // `None` opts out (Unbounded), but the sample still feeds stats.
            if self.fuel_used & MEMORY_SAMPLE_MASK == 0 {
                let used = self.engine.heap_bytes();
                heap_high_water = heap_high_water.max(used);
                if let Some(max) = self.limits.max_memory
                    && used > max
                {
                    let stats = self.finish_stats_with(&mut heap_high_water, used);
                    return (
                        Err(RuntimeError::MemoryLimitExceeded { used, limit: max }),
                        stats,
                    );
                }
            }

            // Fetch and dispatch. The IP must address a validated instruction
            // start; a violation localizes a bad jump to the address that wrote
            // `ip`, before decoding begins mid-instruction.
            #[cfg(debug_assertions)]
            debug_assert!(
                module.is_validated_instruction_start(self.ip),
                "ip {} is not a validated instruction start",
                self.ip
            );
            // Tracing renders from the byte-level decoder so trace output stays
            // identical; the hot path reads the pre-decoded stream.
            if T::ENABLED {
                tracer.trace_instruction(self.ip, &module.decode_instruction(self.ip));
            }

            let result = match module.decoded().instruction_at(self.ip) {
                DecodedInstr::Match(m) => self.exec_match(m, module, tracer),
                DecodedInstr::Call(c) => self.exec_call(c, module, tracer),
                DecodedInstr::RoutedCall(c) => self.exec_routed_call(c, tracer),
                DecodedInstr::SplitCall(c) => self.exec_split_call(c, tracer),
                DecodedInstr::Return(outcome) => self.exec_return(outcome, tracer),
            };

            match result {
                Ok(()) | Err(Signal::Flow(ControlFlow::Backtracked)) => continue,
                Err(Signal::Flow(ControlFlow::Accept)) => {
                    let stats = self.finish_stats(&mut heap_high_water);
                    return (Ok(self.engine.into_journal()), stats);
                }
                Err(Signal::Error(e)) => {
                    let stats = self.finish_stats(&mut heap_high_water);
                    return (Err(e), stats);
                }
            }
        }
    }

    fn finish_stats(&self, heap_high_water: &mut u64) -> RunStats {
        let used = self.engine.heap_bytes();
        self.finish_stats_with(heap_high_water, used)
    }

    fn finish_stats_with(&self, heap_high_water: &mut u64, used: u64) -> RunStats {
        *heap_high_water = (*heap_high_water).max(used);
        RunStats {
            fuel_used: self.fuel_used,
            heap_high_water: *heap_high_water,
        }
    }

    fn exec_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if !m.is_epsilon() {
            self.navigate_and_match(m, module, tracer)?;
        }

        self.finish_match(m, module, tracer)
    }

    /// The post-acceptance half of a Match: run its effects, then branch.
    /// Shared by the dispatch path and the match-retry resume in
    /// [`Self::backtrack`], so a resumed candidate replays exactly what the
    /// original acceptance would have.
    fn finish_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        for &op in module.decoded().effects(&m) {
            self.emit_effect(op, tracer);
        }

        self.branch_to_successors(m, module, tracer)
    }

    fn navigate_and_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let Some(policy) = self.engine.cursor_mut().navigate(m.nav) else {
            if T::ENABLED {
                tracer.trace_nav_failure(m.nav);
            }
            return Err(self.backtrack(module, tracer));
        };
        if T::ENABLED {
            tracer.trace_nav(m.nav, self.engine.node());
        }

        let cont_nav = m.nav.sibling_continuation();
        loop {
            if self.candidate_matches(m, module, tracer) {
                break;
            }
            self.advance_or_backtrack(policy, cont_nav, module, tracer)?;
        }

        if T::ENABLED {
            tracer.trace_match_success(self.engine.node());
        }
        if T::ENABLED
            && let Some(field_id) = m.node_field
        {
            tracer.trace_field_success(field_id);
        }

        self.push_match_retry_if_resumable(m, policy, tracer);

        Ok(())
    }

    /// Accepting a candidate in an engine-owned sibling search is a choice
    /// point: leave a resume checkpoint so a later failure retries the search
    /// past this candidate. Skipped when the search cannot legally step over
    /// the accepted node — either the nav owns no sibling search
    /// ([`Nav::is_sibling_search`]; NFA-level retry loops are compiled with
    /// exact navs precisely to opt out here), or the skip policy does not
    /// admit the node into the pattern's gap (e.g. a named candidate under a
    /// soft anchor is the only legal candidate).
    fn push_match_retry_if_resumable<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        policy: SkipPolicy,
        tracer: &mut T,
    ) {
        if !m.nav.is_sibling_search() || !policy.admits(&self.engine.node()) {
            return;
        }

        let cp = Checkpoint::match_retry(self.engine.checkpoint_state(), u16::from(self.ip));
        self.engine.push_checkpoint(cp);
        if T::ENABLED {
            tracer.trace_checkpoint_created(self.ip);
        }
    }

    /// `p.is_regex` chooses RegexTable over StringTable for `p.value_ref`.
    fn evaluate_predicate(&self, p: DecodedPredicate, module: &Module) -> bool {
        let node = self.engine.node();
        let node_text = node_text(self.source, &node);

        if p.is_regex {
            // The DFAs are deserialized once at module load and reused here;
            // `RegexDfas::is_match` upholds the populated-slot invariant that a
            // module passing load guarantees. Deserializing per evaluation, as
            // this once did, re-validated the whole automaton on every predicate
            // test (issue #426).
            let matched = module
                .regex_dfas()
                .is_match(p.value_ref as usize, node_text);

            match p.op {
                PredicateOp::RegexMatch => matched,
                PredicateOp::RegexNoMatch => !matched,
                _ => unreachable!("non-regex op with is_regex=true"),
            }
        } else {
            let target = module.strings().at(p.value_ref as usize);

            match p.op {
                PredicateOp::Eq => node_text == target,
                PredicateOp::Ne => node_text != target,
                PredicateOp::StartsWith => node_text.starts_with(target),
                PredicateOp::EndsWith => node_text.ends_with(target),
                PredicateOp::Contains => node_text.contains(target),
                _ => unreachable!("regex op with is_regex=false"),
            }
        }
    }

    fn candidate_matches<T: Tracer>(
        &self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> bool {
        let node = self.engine.node();

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
                // kind_id first: it alone rejects most candidates, and each
                // check is an FFI call.
                if node.kind_id() != u16::from(expected) || !node.is_named() {
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
                if node.kind_id() != u16::from(expected) || node.is_named() {
                    if T::ENABLED {
                        tracer.trace_match_failure(node);
                    }
                    return false;
                }
            }
        }

        // `(MISSING …)`: the node kind above is checked as usual, but the node
        // must also be one the parser inserted during error recovery. Missing-ness
        // is an orthogonal runtime flag, not a kind, so it gets its own gate.
        if m.missing && !node.is_missing() {
            if T::ENABLED {
                tracer.trace_match_failure(node);
            }
            return false;
        }

        if let Some(expected) = m.node_field
            && self.engine.cursor().field_id() != Some(expected)
        {
            if T::ENABLED {
                tracer.trace_field_failure(node);
            }
            return false;
        }

        for &field_id in module.decoded().neg_fields(&m) {
            if node.child_by_field_id(u16::from(field_id)).is_some() {
                if T::ENABLED {
                    tracer.trace_neg_field_failure(node, field_id);
                }
                return false;
            }
        }

        if let Some(p) = m.predicate
            && !self.evaluate_predicate(p, module)
        {
            if T::ENABLED {
                tracer.trace_predicate_failure(node);
            }
            return false;
        }

        true
    }

    fn branch_to_successors<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let succs = module.decoded().successors(&m);
        if succs.is_empty() {
            return Err(ControlFlow::Accept.into());
        }

        // Push checkpoints for alternate branches (in reverse order, so LIFO
        // backtracking takes them in priority order).
        if succs.len() > 1 {
            self.engine.push_branches(&succs[1..]);
            if T::ENABLED {
                for _ in &succs[1..] {
                    tracer.trace_checkpoint_created(self.ip);
                }
            }
        }

        self.ip = CodeAddr::from(u16::from(succs[0]));
        Ok(())
    }

    fn exec_call<T: Tracer>(
        &mut self,
        c: DecodedCall,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let skip_policy =
            self.navigate_to_field_with_policy(c.nav, c.node_field, module, tracer)?;

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
            let cp = self.call_retry_checkpoint(self.ip, resume);
            self.engine.push_checkpoint(cp);
            if T::ENABLED {
                tracer.trace_checkpoint_created(self.ip);
            }
        }

        self.enter_callee(c.target, c.next, tracer);
        Ok(())
    }

    fn exec_split_call<T: Tracer>(
        &mut self,
        call: DecodedSplitCall,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if T::ENABLED {
            tracer.trace_call(CodeAddr::from(u16::from(call.target)));
        }
        self.engine
            .enter_split_frame(u16::from(call.matched), u16::from(call.zero));
        self.ip = CodeAddr::from(u16::from(call.target));
        Ok(())
    }

    fn exec_routed_call<T: Tracer>(
        &mut self,
        call: DecodedRoutedCall,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if T::ENABLED {
            tracer.trace_call(CodeAddr::from(u16::from(call.target)));
        }
        self.engine.enter_frame(u16::from(call.next));
        self.ip = CodeAddr::from(u16::from(call.target));
        Ok(())
    }

    /// Push a frame for `target` (returning to `next`) and jump in.
    fn enter_callee<T: Tracer>(
        &mut self,
        target: SuccessorAddr,
        next: SuccessorAddr,
        tracer: &mut T,
    ) {
        if T::ENABLED {
            tracer.trace_call(CodeAddr::from(u16::from(target)));
        }
        self.engine.enter_frame(u16::from(next));
        self.ip = CodeAddr::from(u16::from(target));
    }

    /// Navigate to a field and return the skip policy for retry support.
    ///
    /// Returns `Some(policy)` if navigation was performed, `None` if Stay nav was used.
    fn navigate_to_field_with_policy<T: Tracer>(
        &mut self,
        nav: Nav,
        field: Option<NodeFieldId>,
        module: &Module,
        tracer: &mut T,
    ) -> Result<Option<SkipPolicy>, Signal> {
        if nav == Nav::Stay || nav == Nav::StayExact {
            self.check_field(field, module, tracer)?;
            return Ok(None);
        }

        let Some(policy) = self.engine.cursor_mut().navigate(nav) else {
            if T::ENABLED {
                tracer.trace_nav_failure(nav);
            }
            return Err(self.backtrack(module, tracer));
        };
        if T::ENABLED {
            tracer.trace_nav(nav, self.engine.node());
        }

        let Some(field_id) = field else {
            return Ok(Some(policy));
        };

        let cont_nav = nav.sibling_continuation();
        loop {
            if self.engine.cursor().field_id() == Some(field_id) {
                if T::ENABLED {
                    tracer.trace_field_success(field_id);
                }
                return Ok(Some(policy));
            }
            if T::ENABLED {
                tracer.trace_field_failure(self.engine.node());
            }
            self.advance_or_backtrack(policy, cont_nav, module, tracer)?;
        }
    }

    fn check_field<T: Tracer>(
        &mut self,
        field: Option<NodeFieldId>,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let Some(field_id) = field else {
            return Ok(());
        };
        if self.engine.cursor().field_id() != Some(field_id) {
            if T::ENABLED {
                tracer.trace_field_failure(self.engine.node());
            }
            return Err(self.backtrack(module, tracer));
        }
        if T::ENABLED {
            tracer.trace_field_success(field_id);
        }
        Ok(())
    }

    fn exec_return<T: Tracer>(
        &mut self,
        outcome: ReturnOutcome,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if T::ENABLED {
            tracer.trace_return(outcome);
        }

        // If no frames, we're returning from top-level entrypoint → Accept
        if self.engine.frames_empty() {
            assert_eq!(
                outcome,
                ReturnOutcome::Matched,
                "entrypoint returned through a zero-width call continuation"
            );
            return Err(ControlFlow::Accept.into());
        }

        self.ip = CodeAddr::from(self.engine.exit_frame(outcome));
        Ok(())
    }

    // Loops rather than self-recurses: a run of contiguous retry checkpoints
    // with exhausted siblings (or failed field constraints) is unwound here in one
    // call. The depth of that run is set by the source-tree shape and is decoupled
    // from call depth, so tail-recursion would let untrusted source abort the
    // process on the native stack (Rust does not guarantee TCO). The `continue`
    // paths pop without re-pushing, so the checkpoint stack strictly shrinks until
    // a resume succeeds or it empties — the loop always terminates.
    fn backtrack<T: Tracer>(&mut self, module: &Module, tracer: &mut T) -> Signal {
        'unwind: loop {
            let Some((cp, snapshot)) = self.engine.pop_checkpoint() else {
                return RuntimeError::NoMatch.into();
            };
            if T::ENABLED {
                tracer.trace_backtrack(cp.state.recursion_depth);
            }
            self.engine.restore_checkpoint_state(cp.state, snapshot);

            match cp.resume {
                Resume::Branch => {
                    self.ip = CodeAddr::from(cp.ip);
                    return ControlFlow::Backtracked.into();
                }

                // Call retry: advance to the next candidate satisfying the field
                // constraint, then re-enter the callee. The scan mirrors the
                // navigate-time field search: non-field siblings the policy admits
                // are stepped over, so a retry sees exactly the candidate set the
                // original navigation saw. If siblings are exhausted, keep
                // backtracking to an earlier checkpoint.
                Resume::Call(resume) => {
                    if !self.engine.cursor_mut().continue_search(resume.policy) {
                        continue 'unwind;
                    }
                    if T::ENABLED {
                        tracer.trace_nav(Nav::Down.sibling_continuation(), self.engine.node());
                    }

                    if let Some(field_id) = resume.field {
                        loop {
                            if self.engine.cursor().field_id() == Some(field_id) {
                                if T::ENABLED {
                                    tracer.trace_field_success(field_id);
                                }
                                break;
                            }
                            if T::ENABLED {
                                tracer.trace_field_failure(self.engine.node());
                            }
                            if !self.engine.cursor_mut().continue_search(resume.policy) {
                                continue 'unwind;
                            }
                            if T::ENABLED {
                                tracer.trace_nav(
                                    Nav::Down.sibling_continuation(),
                                    self.engine.node(),
                                );
                            }
                        }
                    }

                    let retry = self.call_retry_checkpoint(CodeAddr::from(cp.ip), resume);
                    self.engine.push_checkpoint(retry);
                    if T::ENABLED {
                        tracer.trace_checkpoint_created(CodeAddr::from(cp.ip));
                    }
                    self.enter_callee(
                        SuccessorAddr::try_from(resume.target)
                            .expect("validated call target is non-zero"),
                        SuccessorAddr::try_from(resume.next)
                            .expect("validated call continuation is non-zero"),
                        tracer,
                    );
                    return ControlFlow::Backtracked.into();
                }

                // Match retry: the checkpoint sits at the accepted-but-failed
                // candidate of an engine-owned sibling search. Step past it (the
                // push gate proved the policy admits it into the gap) and re-run
                // the same instruction's candidate search from there; acceptance
                // replays the match — fresh retry checkpoint, effects, branches —
                // exactly as the dispatch path would.
                Resume::Match => {
                    let DecodedInstr::Match(m) =
                        module.decoded().instruction_at(CodeAddr::from(cp.ip))
                    else {
                        unreachable!("match-retry checkpoint ip must address a Match");
                    };
                    let policy = m.nav.skip_policy();
                    if !self.engine.cursor_mut().continue_search(policy) {
                        continue 'unwind;
                    }
                    let cont_nav = m.nav.sibling_continuation();
                    if T::ENABLED {
                        tracer.trace_nav(cont_nav, self.engine.node());
                    }

                    loop {
                        if self.candidate_matches(m, module, tracer) {
                            break;
                        }
                        if !self.engine.cursor_mut().continue_search(policy) {
                            continue 'unwind;
                        }
                        if T::ENABLED {
                            tracer.trace_nav(cont_nav, self.engine.node());
                        }
                    }

                    if T::ENABLED {
                        tracer.trace_match_success(self.engine.node());
                    }
                    if T::ENABLED
                        && let Some(field_id) = m.node_field
                    {
                        tracer.trace_field_success(field_id);
                    }

                    self.ip = CodeAddr::from(cp.ip);
                    self.push_match_retry_if_resumable(m, policy, tracer);
                    return match self.finish_match(m, module, tracer) {
                        Ok(()) => ControlFlow::Backtracked.into(),
                        Err(signal) => signal,
                    };
                }
            }
        }
    }

    fn advance_or_backtrack<T: Tracer>(
        &mut self,
        policy: SkipPolicy,
        cont_nav: Nav,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if !self.engine.cursor_mut().continue_search(policy) {
            return Err(self.backtrack(module, tracer));
        }
        if T::ENABLED {
            tracer.trace_nav(cont_nav, self.engine.node());
        }
        Ok(())
    }

    fn emit_effect<T: Tracer>(&mut self, op: Effect, tracer: &mut T) {
        use crate::bytecode::EffectSuppression;
        use EffectKind::*;

        let event = match op.kind.suppression() {
            EffectSuppression::Control => match op.kind {
                SuppressBegin => {
                    let was_suppressed = self.engine.suppress_begin();
                    if T::ENABLED {
                        tracer.trace_suppress_control(SuppressBegin, was_suppressed);
                    }
                    return;
                }
                SuppressEnd => {
                    let still_suppressed = self.engine.suppress_end();
                    if T::ENABLED {
                        tracer.trace_suppress_control(SuppressEnd, still_suppressed);
                    }
                    return;
                }
                _ => unreachable!("control metadata only classifies suppression brackets"),
            },
            EffectSuppression::Bypass => match op.kind {
                ScalarMark => {
                    let logged = self.engine.scalar_mark();
                    if T::ENABLED {
                        match logged {
                            Some(event) => tracer.trace_journal_event(event),
                            None => tracer.trace_effect_suppressed(op.kind, op.payload),
                        }
                    }
                    return;
                }
                SpanStartAt => JournalEvent::SpanStart {
                    id: op.payload as u16,
                    node: Some(self.engine.node()),
                },
                SpanStart => JournalEvent::SpanStart {
                    id: op.payload as u16,
                    node: None,
                },
                SpanEnd => JournalEvent::SpanEnd(op.payload as u16),
                _ => unreachable!("bypass metadata only classifies spans and scalar marks"),
            },
            EffectSuppression::Data => {
                let logged = match op.kind {
                    ScalarOpen => self.engine.scalar_open(),
                    StrClose => self.engine.scalar_close_str(),
                    BoolClose => self.engine.scalar_close_bool(op.payload != 0),
                    NodeStr => self.engine.node_str(),
                    NodeBool => self.engine.node_bool(),
                    BoolValue => self.engine.bool_value(op.payload != 0),
                    _ => self.engine.emit_data(|cursor| match op.kind {
                        Node => JournalEvent::Node(cursor.node()),
                        ListOpen => JournalEvent::ListOpen,
                        ArrayPush => JournalEvent::ArrayPush,
                        ListClose => JournalEvent::ListClose,
                        RecordOpen => JournalEvent::RecordOpen,
                        RecordClose => JournalEvent::RecordClose,
                        RecordSet => JournalEvent::RecordSet(op.payload as u16),
                        VariantOpen => JournalEvent::VariantOpen(op.payload as u16),
                        VariantClose => JournalEvent::VariantClose,
                        Absent => JournalEvent::Absent,
                        SuppressBegin | SuppressEnd | SpanStartAt | SpanStart | SpanEnd
                        | ScalarOpen | ScalarMark | StrClose | BoolClose | NodeStr | NodeBool
                        | BoolValue => {
                            unreachable!("metadata routes non-ordinary data effects first")
                        }
                    }),
                };
                if T::ENABLED {
                    match logged {
                        Some(event) => tracer.trace_journal_event(event),
                        None => tracer.trace_effect_suppressed(op.kind, op.payload),
                    }
                }
                return;
            }
        };

        if T::ENABLED {
            tracer.trace_journal_event(&event);
        }
        self.engine.emit_span(event);
    }
}
