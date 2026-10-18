use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use indexmap::IndexSet;

use crate::compiler::analyze::boundary::FirstClass;
use crate::compiler::limits::SatisfiabilityLimits;
use crate::core::grammar::{Grammar, SkeletonStep, SkeletonVariable, StepProjection, VarId};
use crate::core::{NodeFieldId, NodeKindId};

use super::super::model::{KindConstraint, Model, PatternId, PatternKind};
use super::AnalysisLimit;
use super::automaton::{self, ChildAutomaton, ChildMatcher};
use super::budget::Budget;
use super::configurations::{Configuration, Configurations};
use super::facts::GrammarFacts;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::grammar::NodeRealizer;

enum StepClass {
    Visible(VisibleStep),
    HiddenSubtree(HiddenStep),
    HiddenLeaf,
}

impl StepClass {
    fn of(step: &SkeletonStep, grammar: &Grammar) -> Self {
        match step.projection(grammar) {
            StepProjection::Visible { kind, field, body } => StepClass::Visible(VisibleStep {
                kind,
                field,
                realizer: NodeRealizer::of_body(body),
            }),
            StepProjection::Transparent { body, field } => {
                StepClass::HiddenSubtree(HiddenStep { var: body, field })
            }
            StepProjection::HiddenLeaf => StepClass::HiddenLeaf,
        }
    }
}

#[derive(Clone, Copy)]
struct VisibleStep {
    kind: NodeKindId,
    field: Option<NodeFieldId>,
    realizer: NodeRealizer,
}

impl VisibleStep {
    fn effective_field(self, inherited: Option<NodeFieldId>) -> Option<NodeFieldId> {
        self.field.or(inherited)
    }
}

#[derive(Clone, Copy)]
struct HiddenStep {
    var: VarId,
    field: Option<NodeFieldId>,
}

impl HiddenStep {
    fn pushed_field(self, inherited: Option<NodeFieldId>) -> Option<NodeFieldId> {
        self.field.or(inherited)
    }
}

fn field_ok(want: Option<NodeFieldId>, have: Option<NodeFieldId>) -> bool {
    want.is_none() || want == have
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AutomatonId(usize);

impl From<PatternId> for AutomatonId {
    fn from(pattern: PatternId) -> Self {
        Self(pattern.index())
    }
}

type SatKey = (AutomatonId, NodeRealizer);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ThreadKey {
    pattern: AutomatonId,
    hidden_var: VarId,
    input: Configuration,
    inherited_field: Option<NodeFieldId>,
}

struct ProductionThread<'f, 'q> {
    frozen: &'f Frozen<'q>,
    pattern: AutomatonId,
    inherited_field: Option<NodeFieldId>,
}

impl<'f, 'q> ProductionThread<'f, 'q> {
    fn new(
        frozen: &'f Frozen<'q>,
        pattern: AutomatonId,
        inherited_field: Option<NodeFieldId>,
    ) -> Self {
        Self {
            frozen,
            pattern,
            inherited_field,
        }
    }

    fn automaton(&self) -> &ChildAutomaton {
        self.frozen.automaton(self.pattern)
    }

    fn hidden_key(
        &self,
        hidden_var: VarId,
        input: Configuration,
        inherited_field: Option<NodeFieldId>,
    ) -> ThreadKey {
        ThreadKey {
            pattern: self.pattern,
            hidden_var,
            input,
            inherited_field,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Sat(SatKey),
    Thread(ThreadKey),
}

struct Frozen<'a> {
    grammar: &'a Grammar,
    automata: Arc<Vec<Option<ChildAutomaton>>>,
    probe: Option<ChildAutomaton>,
    facts: Arc<GrammarFacts>,
}

impl<'a> Frozen<'a> {
    fn new(grammar: &'a Grammar, facts: Arc<GrammarFacts>) -> Self {
        Self {
            grammar,
            automata: Arc::new(Vec::new()),
            probe: None,
            facts,
        }
    }

    fn automaton(&self, p: AutomatonId) -> &ChildAutomaton {
        if p.0 == self.automata.len() {
            return self
                .probe
                .as_ref()
                .expect("diagnostic probe has a root automaton");
        }

        self.automata[p.0]
            .as_ref()
            .expect("solver only reads completed automata")
    }

    fn with_probe(&self, automaton: ChildAutomaton) -> Self {
        Self {
            grammar: self.grammar,
            automata: Arc::clone(&self.automata),
            probe: Some(automaton),
            facts: Arc::clone(&self.facts),
        }
    }

    fn variable(&self, v: VarId) -> &SkeletonVariable {
        self.grammar
            .structure()
            .variable(v)
            .expect("a VarId from the grammar structure always resolves")
    }

    fn realizers_of(&self, kind: NodeKindId) -> &[NodeRealizer] {
        self.grammar.structure().realizers_of(kind)
    }

    fn consumer_class(&self, kind: KindConstraint) -> FirstClass {
        match kind {
            KindConstraint::Exact(kind) if self.grammar.is_anonymous_node(kind) => {
                FirstClass::Anonymous
            }
            KindConstraint::AnyNode => FirstClass::Either,
            _ => FirstClass::Named,
        }
    }

    fn kind_ok(&self, constraint: KindConstraint, k: NodeKindId) -> bool {
        let grammar = self.grammar;
        match constraint {
            KindConstraint::Exact(id) => id == k,
            KindConstraint::AnyNamed => !grammar.is_anonymous_node(k),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => true,
        }
    }

    fn admits_any_extra(&self, constraint: KindConstraint) -> bool {
        self.facts.admits_any_extra(self.grammar, constraint)
    }

    fn parent_candidate_kinds(&self) -> &[NodeKindId] {
        self.facts.parent_candidate_kinds()
    }

    fn extra_candidates(
        &self,
        constraint: KindConstraint,
    ) -> impl Iterator<Item = NodeKindId> + '_ {
        self.facts.extra_candidates(self.grammar, constraint)
    }
}

pub const DEFAULT_SATISFIABILITY_WORK_BUDGET: u64 = 10_000_000_000;

#[derive(Default)]
struct Solve {
    sat: HashMap<SatKey, bool>,
    thread: HashMap<ThreadKey, Configurations>,
    dependents: HashMap<Key, IndexSet<Key>>,
    current: Option<Key>,
    queue: VecDeque<Key>,
    queued: HashSet<Key>,
    budget: Budget,
}

pub(super) struct SatisfiabilitySolver<'a> {
    model: &'a Model,
    automaton_max_depth: u32,
    frozen: Frozen<'a>,
    solve: Solve,
    patterns: HashMap<PatternId, bool>,
}

impl<'a> SatisfiabilitySolver<'a> {
    pub(super) fn checking(
        grammar: &'a Grammar,
        model: &'a Model,
        limits: SatisfiabilityLimits,
    ) -> Self {
        Self {
            model,
            automaton_max_depth: limits.automaton_max_depth,
            frozen: Frozen::new(grammar, Arc::new(GrammarFacts::from_grammar(grammar))),
            solve: Solve::new(limits.work_budget),
            patterns: HashMap::new(),
        }
    }

    pub(super) fn begin_diagnostics(&mut self, work_budget: u64) {
        self.solve.budget = Budget::new(work_budget);
    }

    pub(super) fn budget(&mut self) -> &mut Budget {
        &mut self.solve.budget
    }

    pub(super) fn copy_automaton(
        &mut self,
        pattern: PatternId,
    ) -> Result<ChildAutomaton, AnalysisLimit> {
        self.frozen
            .automaton(pattern.into())
            .copy(&mut self.solve.budget)
    }

    pub(super) fn child_shapes(
        &mut self,
        kind: NodeKindId,
    ) -> Result<Option<Vec<Vec<super::shapes::Child>>>, AnalysisLimit> {
        super::shapes::children(
            self.frozen.grammar,
            self.frozen.realizers_of(kind),
            &mut self.solve.budget,
        )
    }

    pub(super) fn probe(
        &mut self,
        kind: NodeKindId,
        automaton: ChildAutomaton,
    ) -> Result<bool, AnalysisLimit> {
        self.solve.budget.spend(automaton.state_count() as u64)?;
        self.solve.budget.spend(automaton.work_size() + 1)?;
        for child in automaton.nested_patterns() {
            self.frozen.automaton(child.into());
        }
        let root = AutomatonId(self.frozen.automata.len());
        let frozen = self.frozen.with_probe(automaton);
        let budget = self.solve.budget.remaining();
        let mut probe = Solve::new(budget);
        let result = probe.solve_automaton(&frozen, root, kind);
        self.solve.budget.spend(budget - probe.budget.remaining())?;
        result
    }

    pub(super) fn satisfiable(
        &mut self,
        pattern: PatternId,
        kind: NodeKindId,
    ) -> Result<bool, AnalysisLimit> {
        if self.model.node(pattern).violation.is_some() {
            return Ok(false);
        }

        self.build_pending(pattern)?;
        self.solve_automaton(pattern.into(), kind)
    }

    fn solve_automaton(
        &mut self,
        pattern: AutomatonId,
        kind: NodeKindId,
    ) -> Result<bool, AnalysisLimit> {
        self.solve.solve_automaton(&self.frozen, pattern, kind)
    }

    pub(super) fn wildcard_satisfiable(
        &mut self,
        pattern: PatternId,
    ) -> Result<bool, AnalysisLimit> {
        for index in 0..self.frozen.parent_candidate_kinds().len() {
            let kind = self.frozen.parent_candidate_kinds()[index];
            if self.satisfiable(pattern, kind)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn possible(&mut self, pattern: PatternId) -> Result<bool, AnalysisLimit> {
        if let Some(&result) = self.patterns.get(&pattern) {
            return Ok(result);
        }

        let mut pending = vec![(pattern, 0)];
        let mut visiting = HashSet::from([pattern]);
        while let Some((current, next_child)) = pending.last_mut() {
            self.solve.charge()?;
            let current = *current;
            let kind = &self.model.pattern(current).kind;
            let result = match kind {
                PatternKind::Node(node) => match node.kind {
                    KindConstraint::Exact(kind) => self.satisfiable(current, kind)?,
                    KindConstraint::AnyNamed if node.constrains_children() => {
                        self.wildcard_satisfiable(current)?
                    }
                    _ => true,
                },
                PatternKind::Leaf(_) => true,
                PatternKind::Repeat(repeat) if repeat.kind != QuantifierKind::OneOrMore => true,
                _ => {
                    let alternative = matches!(kind, PatternKind::Alternative(_));
                    let child = match kind {
                        PatternKind::Alternative(children) => children.get(*next_child).copied(),
                        PatternKind::Sequence(items) => match items.get(*next_child) {
                            Some(item) => {
                                let Some(pattern) = item.pattern() else {
                                    *next_child += 1;
                                    continue;
                                };
                                Some(pattern)
                            }
                            None => None,
                        },
                        PatternKind::Field(_, inner) => (*next_child == 0).then_some(*inner),
                        PatternKind::Repeat(repeat) => (*next_child == 0).then_some(repeat.inner),
                        PatternKind::Reference(definition) => {
                            (*next_child == 0).then(|| self.model.definition(*definition))
                        }
                        PatternKind::Node(_) | PatternKind::Leaf(_) => {
                            unreachable!("node and leaf cases were handled above")
                        }
                    };
                    if let Some(child) = child {
                        let child_result = if let Some(&result) = self.patterns.get(&child) {
                            result
                        } else if visiting.contains(&child) {
                            true
                        } else {
                            visiting.insert(child);
                            pending.push((child, 0));
                            continue;
                        };
                        if alternative && child_result {
                            true
                        } else if !alternative && !child_result {
                            false
                        } else {
                            *next_child += 1;
                            continue;
                        }
                    } else {
                        match kind {
                            PatternKind::Alternative(children) => children.is_empty(),
                            _ => true,
                        }
                    }
                }
            };
            self.patterns.insert(current, result);
            visiting.remove(&current);
            pending.pop();
        }
        Ok(self.patterns[&pattern])
    }

    fn build_pending(&mut self, pattern: PatternId) -> Result<(), AnalysisLimit> {
        if self.frozen.automata.is_empty() {
            self.solve.budget.spend(self.model.len() as u64)?;
            Arc::get_mut(&mut self.frozen.automata)
                .expect("primary automata are built before sharing with probes")
                .resize_with(self.model.len(), || None);
        }
        self.solve.budget.charge()?;
        let mut pending = vec![pattern];
        while let Some(pattern) = pending.pop() {
            if self.frozen.automata[pattern.index()].is_some() {
                continue;
            }

            let automaton = automaton::build(
                self.model,
                pattern,
                self.automaton_max_depth,
                &mut self.solve.budget,
            )?;
            self.solve.budget.spend(automaton.state_count() as u64)?;
            self.solve.budget.spend(automaton.work_size())?;
            pending.extend(automaton.nested_patterns());
            Arc::get_mut(&mut self.frozen.automata)
                .expect("primary automata are built before sharing with probes")[pattern.index()] =
                Some(automaton);
        }
        Ok(())
    }
}

impl Solve {
    fn new(budget: u64) -> Self {
        Self {
            budget: Budget::new(budget),
            ..Self::default()
        }
    }

    fn solve_automaton(
        &mut self,
        frozen: &Frozen,
        pattern: AutomatonId,
        kind: NodeKindId,
    ) -> Result<bool, AnalysisLimit> {
        for &realizer in frozen.realizers_of(kind) {
            self.seed(Key::Sat((pattern, realizer)))?;
        }
        while let Some(key) = self.dequeue() {
            if self.recompute(frozen, key)? {
                self.requeue_dependents(key)?;
            }
        }
        Ok(frozen
            .realizers_of(kind)
            .iter()
            .any(|&realizer| self.sat_value((pattern, realizer))))
    }

    fn sat_value(&self, key: SatKey) -> bool {
        *self
            .sat
            .get(&key)
            .expect("a satisfiability key is seeded before it is read")
    }

    fn charge(&mut self) -> Result<(), AnalysisLimit> {
        self.budget.charge()
    }

    fn seed(&mut self, key: Key) -> Result<(), AnalysisLimit> {
        self.charge()?;
        let fresh = match key {
            Key::Sat(k) => !self.sat.contains_key(&k) && self.sat.insert(k, false).is_none(),
            Key::Thread(k) => {
                !self.thread.contains_key(&k)
                    && self.thread.insert(k, Configurations::default()).is_none()
            }
        };
        if fresh {
            self.enqueue(key)?;
        }
        Ok(())
    }

    fn enqueue(&mut self, key: Key) -> Result<(), AnalysisLimit> {
        self.charge()?;
        if self.queued.insert(key) {
            self.queue.push_back(key);
        }
        Ok(())
    }

    fn dequeue(&mut self) -> Option<Key> {
        let key = self.queue.pop_back()?;
        self.queued.remove(&key);
        Some(key)
    }

    fn requeue_dependents(&mut self, key: Key) -> Result<(), AnalysisLimit> {
        let Some(dependents) = self.dependents.get(&key) else {
            return Ok(());
        };
        self.budget.spend(dependents.len() as u64 * 2)?;
        let dependents: Vec<Key> = dependents.iter().copied().collect();
        for dependent in dependents {
            self.enqueue(dependent)?;
        }
        Ok(())
    }

    fn record_read(&mut self, read: Key) -> Result<(), AnalysisLimit> {
        if let Some(current) = self.current {
            self.charge()?;
            self.dependents.entry(read).or_default().insert(current);
        }
        Ok(())
    }

    fn get_sat(&mut self, key: SatKey) -> Result<bool, AnalysisLimit> {
        self.record_read(Key::Sat(key))?;
        self.seed(Key::Sat(key))?;
        Ok(self.sat_value(key))
    }

    fn get_thread(&mut self, key: ThreadKey) -> Result<Configurations, AnalysisLimit> {
        self.record_read(Key::Thread(key))?;
        self.seed(Key::Thread(key))?;
        self.thread
            .get(&key)
            .expect("a thread key is seeded before it is read")
            .copy(&mut self.budget)
    }

    fn recompute(&mut self, frozen: &Frozen, key: Key) -> Result<bool, AnalysisLimit> {
        let previous = self.current.replace(key);
        let changed = match key {
            Key::Sat(k) => {
                let computed = self.compute_sat(frozen, k)?;
                self.sat_value(k) != computed && {
                    self.sat.insert(k, computed);
                    true
                }
            }
            Key::Thread(k) => {
                let computed = self.compute_thread(frozen, k)?;
                let previous = self
                    .thread
                    .get(&k)
                    .expect("a thread key is seeded before recomputation");
                let changed = !previous.equals(&computed, &mut self.budget)?;
                if changed {
                    self.thread.insert(k, computed);
                }
                changed
            }
        };
        self.current = previous;
        Ok(changed)
    }

    fn compute_sat(
        &mut self,
        frozen: &Frozen,
        (pattern, realizer): SatKey,
    ) -> Result<bool, AnalysisLimit> {
        let automaton = frozen.automaton(pattern);
        let accept = automaton.accept();
        if realizer == NodeRealizer::Leaf {
            return self.accepts_empty(automaton);
        }

        let NodeRealizer::Var(variable) = realizer else {
            unreachable!()
        };
        let start =
            Configurations::singleton(Configuration::start(automaton.start()), &mut self.budget)?;
        let thread = ProductionThread::new(frozen, pattern, None);
        for production in &frozen.variable(variable).productions {
            if self
                .thread_production(&thread, production, &start)?
                .contains_state(accept)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn compute_thread(
        &mut self,
        frozen: &Frozen,
        key: ThreadKey,
    ) -> Result<Configurations, AnalysisLimit> {
        let start = Configurations::singleton(key.input, &mut self.budget)?;
        let mut reached = Configurations::default();
        let thread = ProductionThread::new(frozen, key.pattern, key.inherited_field);
        for production in &frozen.variable(key.hidden_var).productions {
            let states = self.thread_production(&thread, production, &start)?;
            reached.union_with(&states, &mut self.budget)?;
        }
        Ok(reached)
    }

    fn thread_production(
        &mut self,
        thread: &ProductionThread<'_, '_>,
        production: &[SkeletonStep],
        start: &Configurations,
    ) -> Result<Configurations, AnalysisLimit> {
        let mut current = self.closure(thread, start)?;
        for step in production {
            if current.is_empty() {
                break;
            }

            current = self.thread_step(thread, &current, step)?;
            current = self.closure(thread, &current)?;
        }
        Ok(current)
    }

    fn thread_step(
        &mut self,
        thread: &ProductionThread<'_, '_>,
        current: &Configurations,
        step: &SkeletonStep,
    ) -> Result<Configurations, AnalysisLimit> {
        match StepClass::of(step, thread.frozen.grammar) {
            StepClass::Visible(visible) => self.thread_visible_step(thread, current, visible),
            StepClass::HiddenSubtree(hidden) => self.thread_hidden_subtree(thread, current, hidden),
            StepClass::HiddenLeaf => current.copy(&mut self.budget),
        }
    }

    fn thread_hidden_subtree(
        &mut self,
        thread: &ProductionThread<'_, '_>,
        current: &Configurations,
        hidden: HiddenStep,
    ) -> Result<Configurations, AnalysisLimit> {
        let pushed = hidden.pushed_field(thread.inherited_field);
        let mut next = Configurations::default();
        for q in current.iter(&mut self.budget)? {
            self.charge()?;
            let reached = self.get_thread(thread.hidden_key(hidden.var, q, pushed))?;
            next.union_with(&reached, &mut self.budget)?;
        }
        Ok(next)
    }

    fn thread_visible_step(
        &mut self,
        thread: &ProductionThread<'_, '_>,
        current: &Configurations,
        visible: VisibleStep,
    ) -> Result<Configurations, AnalysisLimit> {
        let automaton = thread.automaton();
        let effective = visible.effective_field(thread.inherited_field);
        if automaton.negates(effective) {
            return Ok(Configurations::default());
        }
        let node_class = thread.frozen.grammar.node_class(visible.kind);
        let mut next = Configurations::default();
        for q in current.iter(&mut self.budget)? {
            self.charge()?;
            if q.state == automaton.accept()
                && q.boundary.gap_before(FirstClass::Named).admits(node_class)
            {
                next.insert(q, &mut self.budget)?;
            }
            for (matcher, to) in automaton.pattern_edges(q.state) {
                self.charge()?;
                let class = thread.frozen.consumer_class(matcher.kind);
                if automaton.gap(q.state, q.boundary, class).admits(node_class) {
                    next.insert(q, &mut self.budget)?;
                }
                if thread.frozen.kind_ok(matcher.kind, visible.kind)
                    && field_ok(matcher.field, effective)
                    && self.child_sat(matcher, visible.realizer)?
                {
                    next.insert(q.consume(*to, class), &mut self.budget)?;
                }
            }
        }
        Ok(next)
    }

    fn child_sat(
        &mut self,
        matcher: &ChildMatcher,
        realizer: NodeRealizer,
    ) -> Result<bool, AnalysisLimit> {
        match matcher.nested_pattern {
            None => Ok(true),
            Some(nested_pattern) => self.get_sat((nested_pattern.into(), realizer)),
        }
    }

    fn closure(
        &mut self,
        thread: &ProductionThread<'_, '_>,
        set: &Configurations,
    ) -> Result<Configurations, AnalysisLimit> {
        let frozen = thread.frozen;
        let automaton = thread.automaton();
        let mut result = set.copy(&mut self.budget)?;
        let mut stack: Vec<_> = result.iter(&mut self.budget)?.collect();
        while let Some(input) = stack.pop() {
            self.charge()?;
            let Some(boundary) = automaton.advance(input.state, input.boundary) else {
                continue;
            };
            for &to in automaton.eps_edges(input.state) {
                self.charge()?;
                let output = Configuration::new(to, boundary);
                if result.insert(output, &mut self.budget)? {
                    stack.push(output);
                }
            }
            for (matcher, to) in automaton.pattern_edges(input.state) {
                self.charge()?;
                if self.can_consume_extra(frozen, matcher)? {
                    let output = input.consume(*to, frozen.consumer_class(matcher.kind));
                    if result.insert(output, &mut self.budget)? {
                        stack.push(output);
                    }
                }
            }
        }
        Ok(result)
    }

    fn accepts_empty(&mut self, automaton: &ChildAutomaton) -> Result<bool, AnalysisLimit> {
        let start = Configuration::start(automaton.start());
        let mut visited = Configurations::singleton(start, &mut self.budget)?;
        let mut stack = vec![start];
        while let Some(input) = stack.pop() {
            self.charge()?;
            if input.state == automaton.accept() {
                return Ok(true);
            }
            let Some(boundary) = automaton.advance(input.state, input.boundary) else {
                continue;
            };
            for &to in automaton.eps_edges(input.state) {
                self.charge()?;
                let output = Configuration::new(to, boundary);
                if visited.insert(output, &mut self.budget)? {
                    stack.push(output);
                }
            }
        }
        Ok(false)
    }

    fn can_consume_extra(
        &mut self,
        frozen: &Frozen,
        matcher: &ChildMatcher,
    ) -> Result<bool, AnalysisLimit> {
        if matcher.field.is_some() {
            return Ok(false);
        }
        let Some(nested_pattern) = matcher.nested_pattern else {
            return Ok(frozen.admits_any_extra(matcher.kind));
        };
        for extra in frozen.extra_candidates(matcher.kind) {
            for &realizer in frozen.realizers_of(extra) {
                if self.get_sat((nested_pattern.into(), realizer))? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
