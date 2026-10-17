use tree_sitter::{Node, Tree};

use crate::bytecode::{
    CodeAddr, DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, Effect, EffectKind,
    EntryBoundary, Entrypoint, Module, Nav, NodeKindConstraint, PredicateOp, TypeDefKind, TypeKind,
};

use crate::core::NodeFieldId;

use plotnik_runtime::{
    Account, CallResume, Checkpoint, Document, Engine, JournalEvent, LimitExceeded, Meter, PortId,
    ResolvedRuntimeLimits, Resume, RuntimeLimitSpec, SchemaTables, Search, Shared, SkipPolicy,
};

use super::error::{ControlFlow, RuntimeError, Signal};
use super::trace::{NoopTracer, Tracer};

macro_rules! trace {
    ($meter:expr, $tracer:ident, $method:ident($($args:expr),* $(,)?)) => {
        $meter.callback(|| { $tracer.$method($($args),*); $tracer.check() })?
    };
}

/// Resource usage observed during one VM run.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct RunStats {
    /// Fuel consumed by execution. Each matcher dispatch currently
    /// consumes one unit; this is not a stable cross-version performance metric.
    pub fuel_used: u64,
    /// Peak admitted capacity, including metadata and growth overlap.
    pub peak_live_heap_bytes: u64,
}

/// Virtual machine state for query execution.
pub struct VM<'s, 't> {
    pub(crate) engine: Engine<'t>,
    pub(crate) ip: CodeAddr,

    account: Result<Account, LimitExceeded>,

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

struct Application<'q, 's, 't> {
    vm: VM<'s, 't>,
    module: &'q Module,
    entrypoint: Entrypoint,
    schema: Option<Shared<SchemaTables<'q>>>,
}

impl<'q, 's, 't> Application<'q, 's, 't> {
    fn new(vm: VM<'s, 't>, module: &'q Module, entrypoint: Entrypoint) -> Self {
        Self {
            vm,
            module,
            entrypoint,
            schema: None,
        }
    }

    fn apply_budgeted<T: Tracer>(
        &mut self,
        tracer: &mut T,
        budget: &mut Meter<'_>,
    ) -> Result<Option<Document<'q, 't, 's>>, LimitExceeded> {
        let node = self.vm.engine.node();
        let source = self.vm.source;
        if self.schema.is_none() {
            self.schema = Some(super::schema::build(budget, self.module)?);
        }
        let result = self
            .vm
            .run_budgeted(self.module, &self.entrypoint, tracer, budget);
        match result {
            Ok(()) => {}
            Err(RuntimeError::NoMatch) => return Ok(None),
            Err(RuntimeError::Limit(error)) => return Err(error),
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
            self.vm.engine.emit(budget, |_| JournalEvent::Node(node))?;
        }

        Document::new(
            budget,
            self.schema.as_ref().expect("schema built above").clone(),
            self.vm.engine.take_tape(),
            source,
            u32::from(u16::from(self.entrypoint.result_type())),
        )
        .map(Some)
    }

    fn traverse_impl<F>(
        mut self,
        mut apply: F,
    ) -> impl std::iter::FusedIterator<Item = Result<Document<'q, 't, 's>, LimitExceeded>>
    where
        F: FnMut(&mut Self, &mut Meter<'_>) -> Result<Option<Document<'q, 't, 's>>, LimitExceeded>,
    {
        let account = self.vm.account.clone();
        let mut search = Search::new(self.vm.engine.node());
        let mut done = false;
        std::iter::from_fn(move || {
            if done {
                return None;
            }
            let account = match &account {
                Ok(account) => account,
                Err(error) => {
                    done = true;
                    return Some(Err(*error));
                }
            };
            let mut meter = account.meter();
            loop {
                let node = match search.next(&mut meter) {
                    Ok(Some(node)) => node,
                    Ok(None) => {
                        done = true;
                        return None;
                    }
                    Err(error) => {
                        done = true;
                        return Some(Err(error));
                    }
                };
                self.vm.engine.reset(node);
                match apply(&mut self, &mut meter) {
                    Ok(Some(result)) => {
                        search.accepted();
                        return Some(Ok(result));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        done = true;
                        return Some(Err(error));
                    }
                }
            }
        })
        .fuse()
    }
}

impl<'s, 't> VM<'s, 't> {
    fn at_node(source: &'s str, node: Node<'t>, limits: ResolvedRuntimeLimits) -> Self {
        VM {
            engine: Engine::new(node),
            account: Account::new(limits),
            ip: CodeAddr::ZERO,
            source,
        }
    }

    pub fn builder(source: &'s str, tree: &'t Tree) -> VMBuilder<'s, 't> {
        VMBuilder::new(source, tree)
    }

    /// Shared execution and output account, including retained results.
    pub fn account(&self) -> Result<&Account, LimitExceeded> {
        self.account.as_ref().map_err(|error| *error)
    }

    /// Apply an emitted, independently eligible entry at the supplied node.
    /// Match-only entries return that node in the VM's existing value format.
    /// A suitable entry applied to the wrong node is ordinary `None`.
    pub fn apply<'q>(
        self,
        module: &'q Module,
        entrypoint: &Entrypoint,
    ) -> Result<Option<Document<'q, 't, 's>>, LimitExceeded> {
        self.apply_with(module, entrypoint, &mut NoopTracer)
    }

    /// Apply as in [`Self::apply`], tracing the attempt.
    pub fn apply_with<'q, T: Tracer>(
        self,
        module: &'q Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<Option<Document<'q, 't, 's>>, LimitExceeded> {
        let account = self.account.clone()?;
        Application::new(self, module, *entrypoint).apply_budgeted(tracer, &mut account.meter())
    }

    /// Lazily return outermost matches in inclusive, source-order DFS preorder.
    /// A successful candidate prunes its descendants from further search.
    /// All traversal and matching work shares the limits already resolved for
    /// this VM's scope. Recursive captures and collections remain inside each result.
    pub fn traverse<'q>(
        self,
        module: &'q Module,
        entrypoint: Entrypoint,
    ) -> impl std::iter::FusedIterator<Item = Result<Document<'q, 't, 's>, LimitExceeded>> {
        let mut tracer = NoopTracer;
        Application::new(self, module, entrypoint)
            .traverse_impl(move |app, budget| app.apply_budgeted(&mut tracer, budget))
    }

    /// Traverse as in [`Self::traverse`], tracing each attempted candidate, including failures,
    /// only as the iterator is consumed.
    /// Drop the iterator before inspecting the borrowed tracer.
    pub fn traverse_with<'q, T: Tracer>(
        self,
        module: &'q Module,
        entrypoint: Entrypoint,
        tracer: &mut T,
    ) -> impl std::iter::FusedIterator<Item = Result<Document<'q, 't, 's>, LimitExceeded>> {
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
    pub fn execute<'q>(
        self,
        module: &'q Module,
        entrypoint: &Entrypoint,
    ) -> Result<Document<'q, 't, 's>, RuntimeError> {
        self.execute_with(module, entrypoint, &mut NoopTracer)
    }

    /// Execute query with a tracer for debugging.
    ///
    /// The tracer is generic, so `NoopTracer` calls are optimized away
    /// while `PrintTracer` calls collect execution trace.
    ///
    pub fn execute_with<'q, T: Tracer>(
        self,
        module: &'q Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<Document<'q, 't, 's>, RuntimeError> {
        self.apply_with(module, entrypoint, tracer)?
            .ok_or(RuntimeError::NoMatch)
    }

    /// Execute query with a tracer and report run statistics.
    pub fn execute_with_stats<'q, T: Tracer>(
        self,
        module: &'q Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> (Result<Document<'q, 't, 's>, RuntimeError>, RunStats) {
        let account = match self.account.clone() {
            Ok(account) => account,
            Err(error) => {
                return (
                    Err(error.into()),
                    RunStats {
                        fuel_used: 0,
                        peak_live_heap_bytes: 0,
                    },
                );
            }
        };
        let result = Application::new(self, module, *entrypoint)
            .apply_budgeted(tracer, &mut account.meter())
            .map_err(RuntimeError::from)
            .and_then(|result| result.ok_or(RuntimeError::NoMatch));
        let stats = RunStats {
            fuel_used: account.fuel_used(),
            peak_live_heap_bytes: account.memory_peak(),
        };
        (result, stats)
    }

    fn run_budgeted<T: Tracer>(
        &mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
        budget: &mut Meter<'_>,
    ) -> Result<(), RuntimeError> {
        self.ip = entrypoint.target();
        if T::ENABLED {
            let account = budget.account();
            budget.callback(|| {
                tracer.start(account);
                tracer.check()
            })?;
            trace!(budget, tracer, trace_execution_started(self.ip));
        }
        let boundary = entrypoint.boundary();
        self.begin_entry(boundary, tracer, budget)?;

        loop {
            budget.charge(1)?;

            #[cfg(debug_assertions)]
            debug_assert!(
                module.is_validated_instruction_start(self.ip),
                "ip {} is not a validated instruction start",
                self.ip
            );
            if T::ENABLED {
                trace!(
                    budget,
                    tracer,
                    trace_instruction(self.ip, &module.decode_instruction(self.ip))
                );
            }

            let result = match module.decoded().instruction_at(self.ip) {
                DecodedInstr::Match(m) => self.exec_match(m, module, tracer, budget),
                DecodedInstr::Call(c) => self.exec_call(c, module, tracer, budget),
                DecodedInstr::Return(port) => self.exec_return(port, module, tracer, budget),
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
                    self.finish_entry(boundary, tracer, budget)?;
                    self.engine.accepted_tape();
                    break Ok(());
                }
                Err(Signal::Error(e)) => {
                    break Err(e);
                }
            }
        }
    }

    fn begin_entry<T: Tracer>(
        &mut self,
        boundary: EntryBoundary,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), LimitExceeded> {
        if boundary == EntryBoundary::Record {
            self.emit_effect(Effect::new(EffectKind::RecordOpen, 0), tracer, meter)?;
        }
        Ok(())
    }

    fn finish_entry<T: Tracer>(
        &mut self,
        boundary: EntryBoundary,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), LimitExceeded> {
        let kind = match boundary {
            EntryBoundary::Passthrough => return Ok(()),
            EntryBoundary::Node => EffectKind::Node,
            EntryBoundary::Record => EffectKind::RecordClose,
        };
        self.emit_effect(Effect::new(kind, 0), tracer, meter)
    }

    fn exec_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        if !m.is_epsilon() {
            self.navigate_and_match(m, module, tracer, meter)?;
        }

        self.finish_match(m, module, tracer, meter)
    }

    fn finish_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        for &op in module.decoded().effects(&m) {
            self.emit_effect(op, tracer, meter)?;
        }

        self.follow_successors(m, module, tracer, meter)
    }

    fn navigate_and_match<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        let Some(policy) = self.engine.cursor_mut().navigate(meter, m.nav)? else {
            if T::ENABLED {
                trace!(meter, tracer, trace_nav_failure(m.nav));
            }
            return self.backtrack(module, tracer, meter);
        };
        if T::ENABLED {
            trace!(meter, tracer, trace_nav(m.nav, self.engine.node()));
        }

        let cont_nav = m.nav.sibling_continuation();
        loop {
            if self.candidate_matches(m, module, tracer, meter)? {
                break;
            }
            self.advance_or_backtrack(policy, cont_nav, module, tracer, meter)?;
        }

        if T::ENABLED {
            trace!(meter, tracer, trace_match_success(self.engine.node()));
        }
        if T::ENABLED
            && let Some(field_id) = m.node_field
        {
            trace!(meter, tracer, trace_field_success(field_id));
        }

        self.push_match_retry_if_resumable(m, policy, tracer, meter)?;

        Ok(())
    }

    fn push_match_retry_if_resumable<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        policy: SkipPolicy,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), LimitExceeded> {
        if !m.nav.is_sibling_search() || !policy.admits(&self.engine.node()) {
            return Ok(());
        }

        let cp = Checkpoint::match_retry(self.engine.checkpoint_state(), u16::from(self.ip));
        self.engine.push_checkpoint(meter, cp)?;
        if T::ENABLED {
            trace!(meter, tracer, trace_checkpoint_created(self.ip));
        }
        Ok(())
    }

    fn evaluate_predicate(
        &self,
        p: DecodedPredicate,
        module: &Module,
        meter: &mut Meter<'_>,
    ) -> Result<bool, LimitExceeded> {
        let node = self.engine.node();
        let node_text = plotnik_runtime::predicate_text(meter, self.source, &node)?;

        Ok(if p.op.is_regex_op() {
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
        })
    }

    fn candidate_matches<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<bool, LimitExceeded> {
        let node = self.engine.node();

        match m.node_kind {
            NodeKindConstraint::Any => {}
            NodeKindConstraint::Named(None) => {
                if !node.is_named() {
                    if T::ENABLED {
                        trace!(meter, tracer, trace_match_failure(node));
                    }
                    return Ok(false);
                }
            }
            NodeKindConstraint::Named(Some(expected)) => {
                if node.kind_id() != u16::from(expected) || !node.is_named() {
                    if T::ENABLED {
                        trace!(meter, tracer, trace_match_failure(node));
                    }
                    return Ok(false);
                }
            }
            NodeKindConstraint::Anonymous(None) => {
                if node.is_named() {
                    if T::ENABLED {
                        trace!(meter, tracer, trace_match_failure(node));
                    }
                    return Ok(false);
                }
            }
            NodeKindConstraint::Anonymous(Some(expected)) => {
                if node.kind_id() != u16::from(expected) || node.is_named() {
                    if T::ENABLED {
                        trace!(meter, tracer, trace_match_failure(node));
                    }
                    return Ok(false);
                }
            }
        }

        if m.missing && !node.is_missing() {
            if T::ENABLED {
                trace!(meter, tracer, trace_match_failure(node));
            }
            return Ok(false);
        }

        if let Some(expected) = m.node_field
            && !self.engine.cursor_mut().has_field(meter, expected)?
        {
            if T::ENABLED {
                trace!(meter, tracer, trace_field_failure(node));
            }
            return Ok(false);
        }

        for &field_id in module.decoded().neg_fields(&m) {
            meter.charge(1)?;
            if !self.engine.cursor_mut().field_absent(meter, field_id)? {
                if T::ENABLED {
                    trace!(meter, tracer, trace_neg_field_failure(node, field_id));
                }
                return Ok(false);
            }
        }

        if let Some(p) = m.predicate
            && !self.evaluate_predicate(p, module, meter)?
        {
            if T::ENABLED {
                trace!(meter, tracer, trace_predicate_failure(node));
            }
            return Ok(false);
        }

        Ok(true)
    }

    fn follow_successors<T: Tracer>(
        &mut self,
        m: DecodedMatch,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        let succs = module.decoded().successors(&m);
        if succs.is_empty() {
            return Err(ControlFlow::Accept.into());
        }

        if succs.len() > 1 {
            self.engine.push_successors(meter, &succs[1..])?;
            if T::ENABLED {
                for _ in &succs[1..] {
                    trace!(meter, tracer, trace_checkpoint_created(self.ip));
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
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        let call_site = self.ip;
        if !c.caller_owned() {
            self.enter_callee(c.target, call_site, tracer, meter)?;
            return Ok(());
        }

        let skip_policy =
            self.navigate_to_field_with_policy(c.nav, c.node_field, module, tracer, meter)?;

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
            self.engine.push_checkpoint(meter, cp)?;
            if T::ENABLED {
                trace!(meter, tracer, trace_checkpoint_created(call_site));
            }
        }

        self.enter_callee(c.target, call_site, tracer, meter)?;
        Ok(())
    }

    fn enter_callee<T: Tracer>(
        &mut self,
        target: CodeAddr,
        call_site: CodeAddr,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), RuntimeError> {
        self.engine.enter_frame(meter, u16::from(call_site))?;
        if T::ENABLED {
            trace!(meter, tracer, trace_call(target));
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
        meter: &mut Meter<'_>,
    ) -> Result<Option<SkipPolicy>, Signal> {
        if nav == Nav::Stay || nav == Nav::StayExact {
            self.check_field(field, module, tracer, meter)?;
            return Ok(None);
        }

        let Some(policy) = self.engine.cursor_mut().navigate(meter, nav)? else {
            if T::ENABLED {
                trace!(meter, tracer, trace_nav_failure(nav));
            }
            self.backtrack(module, tracer, meter)?;
            unreachable!("backtracking returns a control signal");
        };
        if T::ENABLED {
            trace!(meter, tracer, trace_nav(nav, self.engine.node()));
        }

        let Some(field_id) = field else {
            return Ok(Some(policy));
        };

        let cont_nav = nav.sibling_continuation();
        loop {
            if self.engine.cursor_mut().has_field(meter, field_id)? {
                if T::ENABLED {
                    trace!(meter, tracer, trace_field_success(field_id));
                }
                return Ok(Some(policy));
            }
            if T::ENABLED {
                trace!(meter, tracer, trace_field_failure(self.engine.node()));
            }
            self.advance_or_backtrack(policy, cont_nav, module, tracer, meter)?;
        }
    }

    fn check_field<T: Tracer>(
        &mut self,
        field: Option<NodeFieldId>,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        let Some(field_id) = field else {
            return Ok(());
        };
        if !self.engine.cursor_mut().has_field(meter, field_id)? {
            if T::ENABLED {
                trace!(meter, tracer, trace_field_failure(self.engine.node()));
            }
            return self.backtrack(module, tracer, meter);
        }
        if T::ENABLED {
            trace!(meter, tracer, trace_field_success(field_id));
        }
        Ok(())
    }

    fn exec_return<T: Tracer>(
        &mut self,
        port: PortId,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        if T::ENABLED {
            trace!(meter, tracer, trace_return(port));
        }

        if self.engine.frames_empty() {
            assert_eq!(
                port.to_byte(),
                0,
                "entrypoint returned through a non-zero call port"
            );
            return Err(ControlFlow::Accept.into());
        }

        let call_site = CodeAddr::from(self.engine.exit_frame());
        let DecodedInstr::Call(call) = module.decoded().instruction_at(call_site) else {
            unreachable!("frame call-site token must address a Call");
        };
        self.ip = CodeAddr::from(u16::from(module.decoded().call_return(&call, port)));
        Ok(())
    }

    fn backtrack<T: Tracer>(
        &mut self,
        module: &Module,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        'unwind: loop {
            let Some((cp, snapshot)) = self.engine.pop_checkpoint() else {
                return Err(RuntimeError::NoMatch.into());
            };
            if T::ENABLED {
                trace!(meter, tracer, trace_backtrack(cp.state.recursion_depth));
            }
            self.engine
                .restore_checkpoint_state(meter, cp.state, snapshot)?;

            match cp.resume {
                Resume::Successor => {
                    self.ip = CodeAddr::from(cp.ip);
                    return Err(ControlFlow::Backtracked.into());
                }

                Resume::Call(resume) => {
                    if !self
                        .engine
                        .cursor_mut()
                        .continue_search(meter, resume.policy)?
                    {
                        continue 'unwind;
                    }
                    if T::ENABLED {
                        trace!(
                            meter,
                            tracer,
                            trace_nav(Nav::Down.sibling_continuation(), self.engine.node())
                        );
                    }

                    if let Some(field_id) = resume.field {
                        loop {
                            if self.engine.cursor_mut().has_field(meter, field_id)? {
                                if T::ENABLED {
                                    trace!(meter, tracer, trace_field_success(field_id));
                                }
                                break;
                            }
                            if T::ENABLED {
                                trace!(meter, tracer, trace_field_failure(self.engine.node()));
                            }
                            if !self
                                .engine
                                .cursor_mut()
                                .continue_search(meter, resume.policy)?
                            {
                                continue 'unwind;
                            }
                            if T::ENABLED {
                                trace!(
                                    meter,
                                    tracer,
                                    trace_nav(Nav::Down.sibling_continuation(), self.engine.node(),)
                                );
                            }
                        }
                    }

                    let retry = self.call_retry_checkpoint(CodeAddr::from(cp.ip), resume);
                    self.engine.push_checkpoint(meter, retry)?;
                    if T::ENABLED {
                        trace!(
                            meter,
                            tracer,
                            trace_checkpoint_created(CodeAddr::from(cp.ip))
                        );
                    }
                    self.enter_callee(
                        CodeAddr::from(resume.target),
                        CodeAddr::from(resume.call_site),
                        tracer,
                        meter,
                    )?;
                    return Err(ControlFlow::Backtracked.into());
                }

                Resume::Match => {
                    let DecodedInstr::Match(m) =
                        module.decoded().instruction_at(CodeAddr::from(cp.ip))
                    else {
                        unreachable!("match-retry checkpoint ip must address a Match");
                    };
                    let policy = m.nav.skip_policy();
                    if !self.engine.cursor_mut().continue_search(meter, policy)? {
                        continue 'unwind;
                    }
                    let cont_nav = m.nav.sibling_continuation();
                    if T::ENABLED {
                        trace!(meter, tracer, trace_nav(cont_nav, self.engine.node()));
                    }

                    loop {
                        if self.candidate_matches(m, module, tracer, meter)? {
                            break;
                        }
                        if !self.engine.cursor_mut().continue_search(meter, policy)? {
                            continue 'unwind;
                        }
                        if T::ENABLED {
                            trace!(meter, tracer, trace_nav(cont_nav, self.engine.node()));
                        }
                    }

                    if T::ENABLED {
                        trace!(meter, tracer, trace_match_success(self.engine.node()));
                    }
                    if T::ENABLED
                        && let Some(field_id) = m.node_field
                    {
                        trace!(meter, tracer, trace_field_success(field_id));
                    }

                    self.ip = CodeAddr::from(cp.ip);
                    self.push_match_retry_if_resumable(m, policy, tracer, meter)?;
                    self.finish_match(m, module, tracer, meter)?;
                    return Err(ControlFlow::Backtracked.into());
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
        meter: &mut Meter<'_>,
    ) -> Result<(), Signal> {
        if !self.engine.cursor_mut().continue_search(meter, policy)? {
            return self.backtrack(module, tracer, meter);
        }
        if T::ENABLED {
            trace!(meter, tracer, trace_nav(cont_nav, self.engine.node()));
        }
        Ok(())
    }

    fn emit_effect<T: Tracer>(
        &mut self,
        op: Effect,
        tracer: &mut T,
        meter: &mut Meter<'_>,
    ) -> Result<(), LimitExceeded> {
        use crate::bytecode::EffectSuppression;
        use EffectKind::*;

        let event = match op.kind.suppression() {
            EffectSuppression::Control => match op.kind {
                SuppressBegin => {
                    let was_suppressed = self.engine.suppress_begin();
                    if T::ENABLED {
                        trace!(
                            meter,
                            tracer,
                            trace_suppress_control(SuppressBegin, was_suppressed)
                        );
                    }
                    return Ok(());
                }
                SuppressEnd => {
                    let still_suppressed = self.engine.suppress_end();
                    if T::ENABLED {
                        trace!(
                            meter,
                            tracer,
                            trace_suppress_control(SuppressEnd, still_suppressed)
                        );
                    }
                    return Ok(());
                }
                _ => unreachable!("control metadata only classifies suppression brackets"),
            },
            EffectSuppression::Bypass => match op.kind {
                ScalarMark => {
                    let logged = self.engine.scalar_mark(meter)?;
                    if T::ENABLED {
                        match logged {
                            Some(event) => trace!(meter, tracer, trace_journal_event(&event)),
                            None => {
                                trace!(meter, tracer, trace_effect_suppressed(op.kind, op.payload))
                            }
                        }
                    }
                    return Ok(());
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
                    ScalarOpen => self.engine.scalar_open(meter),
                    TextClose => self.engine.scalar_close_text(meter),
                    BoolClose => self.engine.scalar_close_bool(meter, op.payload != 0),
                    NodeText => self.engine.node_text(meter),
                    NodeBool => self.engine.node_bool(meter),
                    BoolValue => self.engine.bool_value(meter, op.payload != 0),
                    _ => self.engine.emit(meter, |cursor| match op.kind {
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
                }?;
                if T::ENABLED {
                    match logged {
                        Some(event) => trace!(meter, tracer, trace_journal_event(&event)),
                        None => trace!(meter, tracer, trace_effect_suppressed(op.kind, op.payload)),
                    }
                }
                return Ok(());
            }
        };

        if T::ENABLED {
            trace!(meter, tracer, trace_journal_event(&event));
        }
        self.engine.emit_span(meter, event)?;
        Ok(())
    }
}
