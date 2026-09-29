//! The `SAT`/`THREAD` least-fixed-point.
//!
//! Two mutually-recursive judgments over finite, monotone domains:
//!
//! - `SAT(p, realizer)` — a grammar `realizer` (a token `Leaf`, or a non-terminal
//!   `Var`) can realize the child structure query node `p` demands.
//! - `THREAD(p, h, (q, boundary))` — the visible frontier of hidden variable `h`, spliced into
//!   `p`'s child list, drives `A_p` from the input configuration to a set of outputs.
//!
//! They are computed as a least fixed point by a demand-driven worklist: every key
//! starts at bottom (`SAT`→`false`, `THREAD`→∅); recomputing a key records which keys
//! it read, and a key is re-queued only when one of its reads changes. Termination
//! comes from the finite domains and monotonicity (Knaster–Tarski).
//!
//! The domains are finite but not small: a wide query child list yields an automaton
//! with as many states, and threading the grammar through it is quadratic in that
//! width. Construction and solving share a work budget, including collection
//! copying, scans, and dependency storage. Exhaustion returns a resource error.
//! No partial fixed-point value becomes a verdict. This is an analysis-work
//! ceiling, not a wall-clock or byte-allocation limit for the whole compiler.

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

/// How a production step participates in threading.
enum StepClass {
    /// A real child surfacing under `kind`, realized by `realizer`, bound to `field`
    /// when the grammar labels it. A label on this step overrides any pushed down
    /// from a hidden ancestor (the innermost label is the one the runtime attaches).
    Visible(VisibleStep),
    /// A child spliced in without an id of its own: thread through its frontier.
    HiddenSubtree(HiddenStep),
    /// A hidden token: present in the production, absent from the tree.
    HiddenLeaf,
}

impl StepClass {
    /// Classify a step for threading. A supertype is erased in the tree — tree-sitter
    /// never emits a node of the supertype's kind, only one of its subtypes — so a
    /// step surfacing a supertype is threaded through its body, not matched as a node.
    /// Keying the descent off the step's own `body` is what keeps aliased nodes
    /// structurally distinct from their namesakes.
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

/// Whether a matcher demanding field `want` accepts a child whose runtime label is
/// `have`. A bare matcher (`want` is `None`) imposes no field constraint.
fn field_ok(want: Option<NodeFieldId>, have: Option<NodeFieldId>) -> bool {
    want.is_none() || want == have
}

/// Primary automata occupy their pattern's slot. A diagnostic probe appends its
/// own automaton, so recursive edges still target the unchanged primary slots.
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
    /// Part of the key: the same frontier entered under different labels admits
    /// different `field:` matchers, so the memo must keep them apart.
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

/// Data fixed once the automata are built; only `solve` mutates around it. Splitting
/// it from the mutable solve state lets the worklist hold `&Frozen` and `&mut Solve`
/// at once (disjoint borrows) while threading.
struct Frozen<'a> {
    grammar: &'a Grammar,
    automata: Arc<Vec<Option<ChildAutomaton>>>,
    roots: Vec<ChildAutomaton>,
    facts: Arc<GrammarFacts>,
}

impl<'a> Frozen<'a> {
    fn new(grammar: &'a Grammar, facts: Arc<GrammarFacts>) -> Self {
        Self {
            grammar,
            automata: Arc::new(Vec::new()),
            roots: Vec::new(),
            facts,
        }
    }

    fn automaton(&self, p: AutomatonId) -> &ChildAutomaton {
        if p.0 >= self.automata.len() {
            return &self.roots[p.0 - self.automata.len()];
        }

        self.automata[p.0]
            .as_ref()
            .expect("solver only reads completed automata")
    }

    fn with_probe(&self, automaton: ChildAutomaton) -> Self {
        Self {
            grammar: self.grammar,
            automata: Arc::clone(&self.automata),
            roots: vec![automaton],
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

    /// Whether a child-position kind constraint admits visible grammar kind `k`.
    /// Query supertypes are rejected before this pass runs, and grammar supertype
    /// steps are classified as hidden frontiers before matching.
    fn kind_ok(&self, constraint: KindConstraint, k: NodeKindId) -> bool {
        let grammar = self.grammar;
        match constraint {
            KindConstraint::Exact(id) => id == k,
            KindConstraint::AnyNamed => !grammar.is_anonymous_node(k),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => true,
        }
    }

    /// Whether the matcher's kind admits at least one extra kind. The childless fast
    /// path of [`SatisfiabilitySolver::can_consume_extra`] — answered without materializing the
    /// admitted-extra list, which matters on wide wildcard child lists.
    fn admits_any_extra(&self, constraint: KindConstraint) -> bool {
        self.facts.admits_any_extra(self.grammar, constraint)
    }

    /// The concrete named kinds a wildcard parent could be: every named, non-supertype
    /// kind the grammar can surface. A wildcard with children is satisfiable iff one of
    /// these realizes its constraints, including a leaf realizing an empty child list.
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

/// Default ceiling on grammar-analysis work. Charge before expanding patterns,
/// copying and scanning configurations, and storing or following dependencies.
/// Diagnostic refinement has an independent budget of the same size, so its
/// exhaustion cannot replace an established rejection with a resource error.
/// Tunable per query via
/// [`QueryBuilder::with_satisfiability_work_budget`](crate::QueryBuilder::with_satisfiability_work_budget)
/// for the rare case that legitimately needs a wider one.
pub const DEFAULT_SATISFIABILITY_WORK_BUDGET: u64 = 32_000_000;

/// The mutable fixed-point state: memo tables, reverse dependencies, and the worklist.
#[derive(Default)]
struct Solve {
    sat: HashMap<SatKey, bool>,
    thread: HashMap<ThreadKey, Configurations>,
    /// `dependents[k]` are the keys that read `k` — re-queued when `k` changes.
    dependents: HashMap<Key, IndexSet<Key>>,
    /// The key currently being recomputed, so reads attribute to it.
    current: Option<Key>,
    queue: VecDeque<Key>,
    queued: HashSet<Key>,
    /// Work is charged before use. Failure propagates without exposing partial memos.
    budget: Budget,
}

pub(super) struct SatisfiabilitySolver<'a> {
    model: &'a Model,
    automaton_max_depth: u32,
    frozen: Frozen<'a>,
    solve: Solve,
    patterns: HashMap<PatternId, bool>,
}

/// Field-name corrections need the grammar's actual child labels before a valid
/// query model exists. These small probes use the same production solver, with
/// no query patterns or nested obligations.
pub(in crate::compiler::analyze::grammar) struct FieldProbe<'a> {
    frozen: Frozen<'a>,
    solve: Solve,
}

impl<'a> FieldProbe<'a> {
    pub(in crate::compiler::analyze::grammar) fn new(grammar: &'a Grammar, budget: u64) -> Self {
        Self {
            frozen: Frozen::new(grammar, Arc::new(GrammarFacts::from_grammar(grammar))),
            solve: Solve::new(budget),
        }
    }

    pub(in crate::compiler::analyze::grammar) fn accepts(
        &mut self,
        parent: NodeKindId,
        field: NodeFieldId,
        kind: KindConstraint,
    ) -> Result<bool, AnalysisLimit> {
        let automaton =
            ChildAutomaton::child(ChildMatcher::new(kind, Some(field)), &mut self.solve.budget)?;
        self.probe(parent, automaton)
    }

    pub(in crate::compiler::analyze::grammar) fn can_omit(
        &mut self,
        parent: NodeKindId,
        field: NodeFieldId,
    ) -> Result<bool, AnalysisLimit> {
        let automaton = ChildAutomaton::without_field(field, &mut self.solve.budget)?;
        self.probe(parent, automaton)
    }

    fn probe(
        &mut self,
        kind: NodeKindId,
        automaton: ChildAutomaton,
    ) -> Result<bool, AnalysisLimit> {
        self.solve.budget.charge()?;
        let root = AutomatonId(self.frozen.roots.len());
        self.frozen.roots.push(automaton);
        self.solve.solve_automaton(&self.frozen, root, kind)
    }
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

    /// All primary verdicts are complete before diagnostic work starts. Retain
    /// their caches, but charge refinement to a separate budget.
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

    /// Replace only this node's child automaton. Nested nodes keep their original
    /// constraints, including when the same definition occurs in both positions.
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

    /// A completed feasibility result, or a resource failure.
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

    /// Feasibility without a caller's parent. Sibling order and fields are checked
    /// by the enclosing node's automaton. Here sequences require each item, choices
    /// require one live branch, and nullable repeats need not enter their body.
    pub(super) fn possible(&mut self, pattern: PatternId) -> Result<bool, AnalysisLimit> {
        if let Some(&result) = self.patterns.get(&pattern) {
            return Ok(result);
        }

        // A reference chain can multiply the nesting of individually shallow
        // definitions. Keep this walk off the native stack and memoize shared bodies.
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
                            // Sibling recursion stays conservative, as in child automata.
                            // Concrete recursive nodes still use the least fixed point.
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

    /// Finish all transitively referenced child automata before solving. Inserting
    /// each completed automaton before its dependencies also handles recursive nodes.
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

    /// LIFO: a freshly demanded key is recomputed before the key that demanded it
    /// resumes, so a dependency chain settles deepest-first and each fact is revisited
    /// far fewer times than a breadth-first order would force.
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
            // Leaves have no children, not even extras.
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

    /// Thread one production's steps, left to right, through `A_p` from `start`,
    /// returning the reachable state set. `thread.inherited_field` is the field a hidden ancestor
    /// step pushed onto this frontier — `None` for a node's own children.
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
            // A real child of kind `kind`. Each current state either skips it through a
            // gap self-loop, or consumes it through an edge whose kind and field both
            // admit the step. The step's own label wins over an inherited one.
            StepClass::Visible(visible) => self.thread_visible_step(thread, current, visible),
            // Splice the hidden variable's visible frontier in, pushing down the label it
            // inherits: this step's own field if it has one, otherwise the one already
            // inherited (a plain supertype link never relabels what it carries).
            StepClass::HiddenSubtree(hidden) => self.thread_hidden_subtree(thread, current, hidden),
            // A hidden token surfaces nothing and consumes nothing.
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
        // The query asserts this field absent (`-field`); a production binding it
        // gives the node a forbidden child, so this whole path is dead — it can be
        // neither consumed nor skipped past.
        if automaton.negates(effective) {
            return Ok(Configurations::default());
        }
        let node_class = thread.frozen.grammar.node_class(visible.kind);
        let mut next = Configurations::default();
        for q in current.iter(&mut self.budget)? {
            self.charge()?;
            // Control states do not own a search. Each consumer has its own
            // position, so a broad alternative cannot lend its gap to another.
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

    /// Whether the matched child's own structure is realized by `realizer` — trivially
    /// true when the child is childless.
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

    /// Epsilon closure plus optional extra consumption: a query child matching an
    /// extra kind (a `(comment)`) may advance the automaton without a production step,
    /// since the parser may insert an extra anywhere. Extras are optional, so the
    /// closure only grows the reachable set.
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
                    // Consuming an extra satisfies the incoming anchor just as
                    // consuming any other visible child does.
                    let output = input.consume(*to, frozen.consumer_class(matcher.kind));
                    if result.insert(output, &mut self.budget)? {
                        stack.push(output);
                    }
                }
            }
        }
        Ok(result)
    }

    /// A leaf realizes only the empty child sequence. Gaps cannot matter without
    /// children, so this is reachability over epsilon edges alone.
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

    /// Whether a query child matching an extra kind may advance the automaton without a
    /// production step. An extra is conservatively consumable in *any* gap, including
    /// lexically sealed nodes (`(string (comment))`). Proving a gap sealed needs lexer-level
    /// longest-match reasoning our model lacks (not the `IMMEDIATE_TOKEN` fact it looks
    /// like), so we admit and stay sound — extra consumption only grows the reachable set.
    fn can_consume_extra(
        &mut self,
        frozen: &Frozen,
        matcher: &ChildMatcher,
    ) -> Result<bool, AnalysisLimit> {
        // Extras are inserted unfielded, so a `field:` matcher can never be one — letting
        // it "consume" an extra here would skip its field constraint.
        if matcher.field.is_some() {
            return Ok(false);
        }
        let Some(nested_pattern) = matcher.nested_pattern else {
            // Childless matcher: it consumes an extra iff it admits any extra kind. No
            // subtree to realize, so answer in O(1) without building the admitted list —
            // the hot path on wide wildcard child lists.
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
