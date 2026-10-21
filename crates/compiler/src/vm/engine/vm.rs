use tree_sitter::{Node, Tree};

use crate::bytecode::{
    CodeAddr, DecodedAction, DecodedCall, DecodedCandidate, DecodedInstr, DecodedMatch,
    DecodedPredicate, Effect, EffectKind, EntryBoundary, Entrypoint, Module, Nav, PredicateOp,
    TypeDefKind, TypeKind,
};

use crate::core::{Colors, NodeFieldId};

use plotnik_runtime::{
    CallResume, Checkpoint, Engine, ExecutionBudget, JournalEvent, LimitExceeded, MatchJournal,
    PortId, ResolvedRuntimeLimits, Resume, RuntimeLimitSpec, SkipPolicy, Traversal,
};

use super::error::{ControlFlow, RuntimeError, Signal};
use super::materializer::ValueMaterializer;
use super::trace::{NoopTracer, Tracer};
use super::value::{NodeValue, Value, node_text};

const BINARY_SEARCH_MIN_CASES: usize = 48;

/// Resource usage observed during one VM run.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RunStats {
    /// Fuel consumed by execution. Each execution operation
    /// consumes one unit; this is not a stable cross-version performance metric.
    pub fuel_used: u64,
    /// Peak estimated runtime memory for occupied entries, sampled during execution
    /// and at run exit. Excludes spare allocation capacity.
    pub peak_live_heap_bytes: u64,
}

trait StatsObserver: Default {
    const ENABLED: bool;
    type Report;

    fn observe_heap(&mut self, used: u64);
    fn finish(self, budget: &ExecutionBudget) -> Self::Report;
}

#[derive(Default)]
struct NoStats;

impl StatsObserver for NoStats {
    const ENABLED: bool = false;
    type Report = ();

    fn observe_heap(&mut self, _used: u64) {}

    fn finish(self, _budget: &ExecutionBudget) {}
}

#[derive(Default)]
struct CollectStats {
    peak_live_heap_bytes: u64,
}

impl StatsObserver for CollectStats {
    const ENABLED: bool = true;
    type Report = RunStats;

    fn observe_heap(&mut self, used: u64) {
        self.peak_live_heap_bytes = self.peak_live_heap_bytes.max(used);
    }

    fn finish(self, budget: &ExecutionBudget) -> RunStats {
        RunStats {
            fuel_used: budget.fuel_used(),
            peak_live_heap_bytes: self.peak_live_heap_bytes,
        }
    }
}

/// Virtual machine state for query execution.
pub struct VM<'s, 't> {
    pub(crate) engine: Engine<'t>,
    pub(crate) ip: CodeAddr,

    pub(crate) limits: ResolvedRuntimeLimits,

    pub(crate) source: &'s str,
}

pub struct VMBuilder<'s, 't> {
    source: &'s str,
    node: Node<'t>,
    spec: RuntimeLimitSpec,
}

impl<'s, 't> VMBuilder<'s, 't> {
    pub fn new(source: &'s str, tree: &'t Tree) -> Self {
        Self::at_node(source, tree.root_node())
    }

    /// Start at a supplied node. Matching cannot leave this node's subtree.
    pub fn at_node(source: &'s str, node: Node<'t>) -> Self {
        Self {
            source,
            node,
            spec: RuntimeLimitSpec::default(),
        }
    }

    /// Set the runtime limit policy. `Auto` limits are sized from the source
    /// scope's node count when [`Self::build`] resolves them.
    pub fn limits(mut self, spec: RuntimeLimitSpec) -> Self {
        self.spec = spec;
        self
    }

    /// Build the VM, resolving `Auto` limits once against the scope's node count.
    pub fn build(self) -> VM<'s, 't> {
        let source_nodes = u32::try_from(self.node.descendant_count()).unwrap_or(u32::MAX);
        VM::at_node(self.source, self.node, self.spec.resolve(source_nodes))
    }
}

struct Application<'s, 't> {
    vm: VM<'s, 't>,
    module: &'s Module,
    entrypoint: Entrypoint,
    materializer: Option<ValueMaterializer<'s, 't>>,
}

impl<'s, 't> Application<'s, 't> {
    fn new(vm: VM<'s, 't>, module: &'s Module, entrypoint: Entrypoint) -> Self {
        Self {
            vm,
            module,
            entrypoint,
            materializer: None,
        }
    }

    fn apply_budgeted<T: Tracer>(
        &mut self,
        tracer: &mut T,
        budget: &mut ExecutionBudget,
    ) -> Result<Option<Value<'s, 't>>, LimitExceeded> {
        let node = self.vm.engine.node();
        let source = self.vm.source;
        let (result, _) =
            self.vm
                .run_budgeted::<T, NoStats>(self.module, &self.entrypoint, tracer, budget);
        match result {
            Ok(()) => {}
            Err(RuntimeError::NoMatch) => return Ok(None),
            Err(RuntimeError::OutOfFuel(limit)) => return Err(LimitExceeded::OutOfFuel(limit)),
            Err(RuntimeError::MemoryLimitExceeded { used, limit }) => {
                return Err(LimitExceeded::Memory { used, limit });
            }
            Err(RuntimeError::CallFrame(error)) => return Err(error.into()),
        }

        let result_type = self
            .module
            .types()
            .get(self.entrypoint.result_type())
            .expect("entrypoint result type belongs to its module");
        if matches!(
            result_type.decode(),
            TypeDefKind::Primitive(TypeKind::NoValue)
        ) {
            return Ok(Some(Value::Node(NodeValue::from_node(node, source))));
        }

        let materializer = self
            .materializer
            .get_or_insert_with(|| ValueMaterializer::new(source, self.module));
        Ok(Some(materializer.materialize_verified(
            &self.entrypoint,
            self.vm.engine.journal().output_events(),
            Colors::new(false),
        )))
    }

    fn traverse_impl<F>(
        mut self,
        mut apply: F,
    ) -> impl std::iter::FusedIterator<Item = Result<Value<'s, 't>, LimitExceeded>>
    where
        F: FnMut(&mut Self, &mut ExecutionBudget) -> Result<Option<Value<'s, 't>>, LimitExceeded>,
    {
        Traversal::new(
            self.vm.engine.node(),
            ExecutionBudget::new(self.vm.limits),
            move |node, budget: &mut ExecutionBudget| {
                self.vm.engine.reset(node);
                apply(&mut self, budget)
            },
        )
    }
}

impl<'s, 't> VM<'s, 't> {
    fn at_node(source: &'s str, node: Node<'t>, limits: ResolvedRuntimeLimits) -> Self {
        VM {
            engine: Engine::new(node.walk()),
            ip: CodeAddr::ZERO,
            limits,
            source,
        }
    }

    pub fn builder(source: &'s str, tree: &'t Tree) -> VMBuilder<'s, 't> {
        VMBuilder::new(source, tree)
    }

    /// Apply an emitted, independently eligible entry at the supplied node.
    /// Match-only entries return that node in the VM's existing value format.
    /// A suitable entry applied to the wrong node is ordinary `None`.
    pub fn apply(
        self,
        module: &'s Module,
        entrypoint: &Entrypoint,
    ) -> Result<Option<Value<'s, 't>>, LimitExceeded> {
        self.apply_with(module, entrypoint, &mut NoopTracer)
    }

    /// Apply as in [`Self::apply`], tracing the attempt.
    pub fn apply_with<T: Tracer>(
        self,
        module: &'s Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<Option<Value<'s, 't>>, LimitExceeded> {
        let mut budget = ExecutionBudget::new(self.limits);
        Application::new(self, module, *entrypoint).apply_budgeted(tracer, &mut budget)
    }

    /// Lazily return outermost matches in inclusive, source-order DFS preorder.
    /// A successful candidate prunes its descendants from further search.
    /// All traversal and matching work shares the limits already resolved for
    /// this VM's scope. Recursive captures and collections remain inside each result.
    pub fn traverse(
        self,
        module: &'s Module,
        entrypoint: Entrypoint,
    ) -> impl std::iter::FusedIterator<Item = Result<Value<'s, 't>, LimitExceeded>> {
        let mut tracer = NoopTracer;
        Application::new(self, module, entrypoint)
            .traverse_impl(move |app, budget| app.apply_budgeted(&mut tracer, budget))
    }

    /// Traverse as in [`Self::traverse`], tracing each attempted candidate, including failures,
    /// only as the iterator is consumed.
    /// Drop the iterator before inspecting the borrowed tracer.
    pub fn traverse_with<T: Tracer>(
        self,
        module: &'s Module,
        entrypoint: Entrypoint,
        tracer: &mut T,
    ) -> impl std::iter::FusedIterator<Item = Result<Value<'s, 't>, LimitExceeded>> {
        Application::new(self, module, entrypoint)
            .traverse_impl(move |app, budget| app.apply_budgeted(tracer, budget))
    }

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
        self.execute_impl::<T, NoStats>(module, entrypoint, tracer)
            .0
    }

    /// Execute query with a tracer and report run statistics.
    pub fn execute_with_stats<T: Tracer>(
        self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> (Result<MatchJournal<'t>, RuntimeError>, RunStats) {
        self.execute_impl::<T, CollectStats>(module, entrypoint, tracer)
    }

    fn execute_impl<T: Tracer, S: StatsObserver>(
        mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> (Result<MatchJournal<'t>, RuntimeError>, S::Report) {
        let mut budget = ExecutionBudget::new(self.limits);
        let (result, report) = self.run_budgeted::<T, S>(module, entrypoint, tracer, &mut budget);
        (result.map(|()| self.engine.into_journal()), report)
    }

    fn run_budgeted<T: Tracer, S: StatsObserver>(
        &mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
        budget: &mut ExecutionBudget,
    ) -> (Result<(), RuntimeError>, S::Report) {
        self.ip = entrypoint.target();
        if T::ENABLED {
            tracer.trace_execution_started(self.ip);
        }
        let boundary = entrypoint.boundary();
        self.begin_entry(boundary, tracer);

        let mut stats = S::default();
        if S::ENABLED {
            stats.observe_heap(self.engine.heap_bytes());
        }

        let result = loop {
            if let Err(error) = budget.charge() {
                let LimitExceeded::OutOfFuel(limit) = error else {
                    unreachable!("charging fuel only reports fuel exhaustion");
                };
                break Err(RuntimeError::OutOfFuel(limit));
            }

            if let Err(error) = self.sample_memory(budget, &mut stats) {
                break Err(error);
            }

            #[cfg(debug_assertions)]
            debug_assert!(
                module.is_validated_instruction_start(self.ip),
                "ip {} is not a validated instruction start",
                self.ip
            );
            if T::ENABLED {
                tracer.trace_instruction(self.ip, &module.decode_instruction(self.ip));
            }

            let result = match module.decoded().instruction_at(self.ip) {
                DecodedInstr::Switch(switch) => {
                    let node = self.engine.node();
                    let kind = node.kind_id();
                    let entries = module.decoded().switch_entries(&switch);
                    let entry = if entries.len() >= BINARY_SEARCH_MIN_CASES {
                        match entries.binary_search_by_key(&kind, |entry| u16::from(entry.kind)) {
                            Ok(index) => Some(&entries[index]),
                            Err(_) => None,
                        }
                    } else {
                        entries.iter().find(|entry| u16::from(entry.kind) == kind)
                    };
                    let action = entry.map(|entry| module.decoded().switch_action(entry));
                    if T::ENABLED {
                        tracer.trace_switch(self.ip, node, action.map(|action| action.entry));
                    }
                    if let Some(action) = action {
                        self.exec_selected(action, node, module, tracer, budget, &mut stats)
                    } else {
                        Err(self.backtrack(module, tracer))
                    }
                }
                DecodedInstr::Match(m) => self.exec_match(m, module, tracer),
                DecodedInstr::Call(c) => self.exec_call(c, module, tracer),
                DecodedInstr::Return(port) => self.exec_return(port, module, tracer),
                DecodedInstr::Interior => {
                    unreachable!(
                        "decoded bytecode lookup returned an interior word at instruction pointer \
                         {:?} after `instruction_at` accepted the address",
                        self.ip
                    )
                }
            };

            match result {
                Ok(()) | Err(Signal::Flow(ControlFlow::Backtracked)) => continue,
                Err(Signal::Flow(ControlFlow::Accept)) => {
                    self.finish_entry(boundary, tracer);
                    self.engine.accepted_journal();
                    break Ok(());
                }
                Err(Signal::Error(e)) => {
                    break Err(e);
                }
            }
        };

        if S::ENABLED {
            stats.observe_heap(self.engine.heap_bytes());
        }

        (result, stats.finish(budget))
    }

    fn sample_memory<S: StatsObserver>(
        &self,
        budget: &mut ExecutionBudget,
        stats: &mut S,
    ) -> Result<(), RuntimeError> {
        if !(S::ENABLED || self.limits.max_memory.is_some()) || !budget.memory_sample_due() {
            return Ok(());
        }

        let used = self.engine.heap_bytes();
        if S::ENABLED {
            stats.observe_heap(used);
        }
        if let Some(limit) = self.limits.max_memory
            && used > limit
        {
            return Err(RuntimeError::MemoryLimitExceeded { used, limit });
        }

        Ok(())
    }

    #[inline]
    fn exec_selected<T: Tracer, S: StatsObserver>(
        &mut self,
        action: &DecodedAction,
        node: Node<'t>,
        module: &Module,
        tracer: &mut T,
        budget: &mut ExecutionBudget,
        stats: &mut S,
    ) -> Result<(), Signal> {
        for prefix in &action.prefix {
            if S::ENABLED || self.limits.max_memory.is_some() {
                budget.account_memory_work();
                self.sample_memory(budget, stats)?;
            }
            if T::ENABLED {
                self.ip = prefix.ip;
                tracer.trace_instruction(self.ip, &module.decode_instruction(self.ip));
            }
            for &effect in module.decoded().prefix_effects(prefix) {
                self.emit_effect(effect, tracer);
            }
        }

        if S::ENABLED || self.limits.max_memory.is_some() {
            budget.account_memory_work();
            self.sample_memory(budget, stats)?;
        }
        self.ip = action.matched_at;
        if T::ENABLED {
            tracer.trace_instruction(self.ip, &module.decode_instruction(self.ip));
            tracer.trace_nav(Nav::StayExact, node);
        }
        if !self.residual_matches(action.matched.candidate, node, module, tracer) {
            if T::ENABLED {
                tracer.trace_nav_failure(Nav::StayExact.sibling_continuation());
            }
            return Err(self.backtrack(module, tracer));
        }

        if T::ENABLED {
            tracer.trace_match_success(node, self.engine.cursor().field_id());
            if let Some(field) = action.matched.candidate.node_field {
                tracer.trace_field_success(field);
            }
        }
        self.finish_match(action.matched, module, tracer)
    }

    fn begin_entry<T: Tracer>(&mut self, boundary: EntryBoundary, tracer: &mut T) {
        if boundary == EntryBoundary::Record {
            self.emit_effect(Effect::new(EffectKind::RecordOpen, 0), tracer);
        }
    }

    fn finish_entry<T: Tracer>(&mut self, boundary: EntryBoundary, tracer: &mut T) {
        let kind = match boundary {
            EntryBoundary::Passthrough => return,
            EntryBoundary::Node => EffectKind::Node,
            EntryBoundary::Record => EffectKind::RecordClose,
        };
        self.emit_effect(Effect::new(kind, 0), tracer);
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

    fn finish_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        for &op in module.decoded().effects(&m) {
            self.emit_effect(op, tracer);
        }

        self.follow_successors(m, module, tracer)
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
            tracer.trace_match_success(self.engine.node(), self.engine.cursor().field_id());
        }
        if T::ENABLED
            && let Some(field_id) = m.candidate.node_field
        {
            tracer.trace_field_success(field_id);
        }

        self.push_match_retry_if_resumable(m, policy, tracer);

        Ok(())
    }

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

    fn evaluate_predicate(&self, p: DecodedPredicate, module: &Module) -> bool {
        let node = self.engine.node();
        let node_text = node_text(self.source, &node);

        if p.op.is_regex_op() {
            let matched = module
                .regex_dfas()
                .is_match(p.value_ref as usize, node_text);

            match p.op {
                PredicateOp::RegexMatch => matched,
                PredicateOp::RegexNoMatch => !matched,
                _ => unreachable!("string operator in regex evaluation"),
            }
        } else {
            let target = module.strings().at(p.value_ref as usize);

            match p.op {
                PredicateOp::Eq => node_text == target,
                PredicateOp::Ne => node_text != target,
                PredicateOp::StartsWith => node_text.starts_with(target),
                PredicateOp::EndsWith => node_text.ends_with(target),
                PredicateOp::Contains => node_text.contains(target),
                _ => unreachable!("regex operator in string evaluation"),
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

        if let Some(expected) = m.kind
            && node.kind_id() != u16::from(expected)
        {
            if T::ENABLED {
                tracer.trace_match_failure(node, self.engine.cursor().field_id());
            }
            return false;
        }

        self.residual_matches(m.candidate, node, module, tracer)
    }

    #[inline]
    fn residual_matches<T: Tracer>(
        &self,
        candidate: DecodedCandidate,
        node: Node<'t>,
        module: &Module,
        tracer: &mut T,
    ) -> bool {
        if let Some(named) = candidate.named
            && node.is_named() != named
        {
            if T::ENABLED {
                tracer.trace_match_failure(node, self.engine.cursor().field_id());
            }
            return false;
        }

        if candidate.missing && !node.is_missing() {
            if T::ENABLED {
                tracer.trace_match_failure(node, self.engine.cursor().field_id());
            }
            return false;
        }

        if let Some(expected) = candidate.node_field
            && self.engine.cursor().field_id() != Some(expected)
        {
            if T::ENABLED {
                tracer.trace_field_failure(node, self.engine.cursor().field_id());
            }
            return false;
        }

        for &field_id in module.decoded().neg_fields(&candidate) {
            if node.child_by_field_id(u16::from(field_id)).is_some() {
                if T::ENABLED {
                    tracer.trace_neg_field_failure(node, self.engine.cursor().field_id(), field_id);
                }
                return false;
            }
        }

        if let Some(p) = candidate.predicate
            && !self.evaluate_predicate(p, module)
        {
            if T::ENABLED {
                tracer.trace_predicate_failure(node, self.engine.cursor().field_id());
            }
            return false;
        }

        true
    }

    fn follow_successors<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        let succs = module.decoded().successors(&m);
        if succs.is_empty() {
            return Err(ControlFlow::Accept.into());
        }

        if succs.len() > 1 {
            self.engine.push_successors(&succs[1..]);
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
        let call_site = self.ip;
        if !c.caller_owned() {
            self.enter_callee(c.target, call_site, tracer)?;
            return Ok(());
        }

        let skip_policy =
            self.navigate_to_field_with_policy(c.nav, c.node_field, module, tracer)?;

        if let Some(policy) = skip_policy
            && policy != SkipPolicy::Exact
        {
            let resume = CallResume {
                target: u16::from(c.target),
                call_site: u16::from(call_site),
                field: c.node_field,
                policy,
            };
            let cp = self.call_retry_checkpoint(call_site, resume);
            self.engine.push_checkpoint(cp);
            if T::ENABLED {
                tracer.trace_checkpoint_created(call_site);
            }
        }

        self.enter_callee(c.target, call_site, tracer)?;
        Ok(())
    }

    fn enter_callee<T: Tracer>(
        &mut self,
        target: CodeAddr,
        call_site: CodeAddr,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        self.engine.enter_frame(u16::from(call_site))?;
        if T::ENABLED {
            tracer.trace_call(target);
        }
        self.ip = target;
        Ok(())
    }

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
            if T::ENABLED {
                tracer.trace_match_success(self.engine.node(), self.engine.cursor().field_id());
            }
            return Ok(Some(policy));
        };

        let cont_nav = nav.sibling_continuation();
        loop {
            if self.engine.cursor().field_id() == Some(field_id) {
                if T::ENABLED {
                    tracer.trace_match_success(self.engine.node(), self.engine.cursor().field_id());
                    tracer.trace_field_success(field_id);
                }
                return Ok(Some(policy));
            }
            if T::ENABLED {
                tracer.trace_field_failure(self.engine.node(), self.engine.cursor().field_id());
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
            if T::ENABLED {
                tracer.trace_match_success(self.engine.node(), self.engine.cursor().field_id());
            }
            return Ok(());
        };
        if self.engine.cursor().field_id() != Some(field_id) {
            if T::ENABLED {
                tracer.trace_field_failure(self.engine.node(), self.engine.cursor().field_id());
            }
            return Err(self.backtrack(module, tracer));
        }
        if T::ENABLED {
            tracer.trace_match_success(self.engine.node(), self.engine.cursor().field_id());
            tracer.trace_field_success(field_id);
        }
        Ok(())
    }

    fn exec_return<T: Tracer>(
        &mut self,
        port: PortId,
        module: &Module,
        tracer: &mut T,
    ) -> Result<(), Signal> {
        if self.engine.frames_empty() {
            assert_eq!(
                port.to_byte(),
                0,
                "entrypoint returned through a non-zero call port"
            );
            if T::ENABLED {
                tracer.trace_return(port, None);
            }
            return Err(ControlFlow::Accept.into());
        }

        let call_site = CodeAddr::from(self.engine.exit_frame());
        let DecodedInstr::Call(call) = module.decoded().instruction_at(call_site) else {
            unreachable!("frame call-site token must address a Call");
        };
        let target = CodeAddr::from(u16::from(module.decoded().call_return(&call, port)));
        if T::ENABLED {
            tracer.trace_return(port, Some(target));
        }
        self.ip = target;
        Ok(())
    }

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
                Resume::Successor => {
                    self.ip = CodeAddr::from(cp.ip);
                    return ControlFlow::Backtracked.into();
                }

                Resume::Call(resume) => {
                    if T::ENABLED {
                        tracer.trace_instruction(
                            CodeAddr::from(cp.ip),
                            &module.decode_instruction(CodeAddr::from(cp.ip)),
                        );
                    }
                    if !self.engine.cursor_mut().continue_search(resume.policy) {
                        if T::ENABLED {
                            tracer.trace_nav_failure(Nav::Next);
                        }
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
                                tracer.trace_field_failure(
                                    self.engine.node(),
                                    self.engine.cursor().field_id(),
                                );
                            }
                            if !self.engine.cursor_mut().continue_search(resume.policy) {
                                if T::ENABLED {
                                    tracer.trace_nav_failure(Nav::Next);
                                }
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

                    if T::ENABLED {
                        tracer.trace_match_success(
                            self.engine.node(),
                            self.engine.cursor().field_id(),
                        );
                    }
                    let retry = self.call_retry_checkpoint(CodeAddr::from(cp.ip), resume);
                    self.engine.push_checkpoint(retry);
                    if T::ENABLED {
                        tracer.trace_checkpoint_created(CodeAddr::from(cp.ip));
                    }
                    if let Err(error) = self.enter_callee(
                        CodeAddr::from(resume.target),
                        CodeAddr::from(resume.call_site),
                        tracer,
                    ) {
                        return error.into();
                    }
                    return ControlFlow::Backtracked.into();
                }

                Resume::Match => {
                    let DecodedInstr::Match(m) =
                        module.decoded().instruction_at(CodeAddr::from(cp.ip))
                    else {
                        unreachable!("match-retry checkpoint ip must address a Match");
                    };
                    if T::ENABLED {
                        tracer.trace_instruction(
                            CodeAddr::from(cp.ip),
                            &module.decode_instruction(CodeAddr::from(cp.ip)),
                        );
                    }
                    let policy = m.nav.skip_policy();
                    if !self.engine.cursor_mut().continue_search(policy) {
                        if T::ENABLED {
                            tracer.trace_nav_failure(m.nav.sibling_continuation());
                        }
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
                            if T::ENABLED {
                                tracer.trace_nav_failure(cont_nav);
                            }
                            continue 'unwind;
                        }
                        if T::ENABLED {
                            tracer.trace_nav(cont_nav, self.engine.node());
                        }
                    }

                    if T::ENABLED {
                        tracer.trace_match_success(
                            self.engine.node(),
                            self.engine.cursor().field_id(),
                        );
                    }
                    if T::ENABLED
                        && let Some(field_id) = m.candidate.node_field
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
            if T::ENABLED {
                tracer.trace_nav_failure(cont_nav);
            }
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
            EffectSuppression::Output => {
                let logged = match op.kind {
                    ScalarOpen => self.engine.scalar_open(),
                    TextClose => self.engine.scalar_close_text(),
                    BoolClose => self.engine.scalar_close_bool(op.payload != 0),
                    NodeText => self.engine.node_text(),
                    NodeBool => self.engine.node_bool(),
                    BoolValue => self.engine.bool_value(op.payload != 0),
                    _ => self.engine.emit_output_event(|cursor| match op.kind {
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
                        | ScalarOpen | ScalarMark | TextClose | BoolClose | NodeText | NodeBool
                        | BoolValue => {
                            unreachable!("metadata routes non-ordinary output effects first")
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
