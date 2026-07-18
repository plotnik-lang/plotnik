//! The `SAT`/`THREAD` least-fixed-point.
//!
//! Two mutually-recursive judgments over finite, monotone domains:
//!
//! - `SAT(p, realizer)` — a grammar `realizer` (a token `Leaf`, or a non-terminal
//!   `Var`) can realize the child structure query node `p` demands.
//! - `THREAD(p, h, q)` — the visible frontier of hidden variable `h`, spliced into
//!   `p`'s child list, drives `A_p` from state `q` to the returned set of states.
//!
//! They are computed as a least fixed point by a demand-driven worklist: every key
//! starts at bottom (`SAT`→`false`, `THREAD`→∅); recomputing a key records which keys
//! it read, and a key is re-queued only when one of its reads changes. Termination
//! comes from the finite domains and monotonicity (Knaster–Tarski).
//!
//! The domains are finite but not small: a wide query child list yields an automaton
//! with as many states, and threading the grammar through it is quadratic in that
//! width. So the solve carries a work budget; once the per-state work exceeds it the
//! solve gives up, every pending verdict reads as *accept* (the sound default), and
//! the pass rejects the whole query as too complex rather than spend unbounded time.
//! The budget bounds work, not wall-clock, so the cut-off is the same on every
//! machine — a slow host merely takes proportionally longer to reach it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use indexmap::IndexSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::{AnchorSemantics, GapClass};
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::NamedNodePattern;
use crate::core::grammar::{Grammar, SkeletonStep, SkeletonVariable, StepProjection, VarId};
use crate::core::{NodeFieldId, NodeKindId};

use super::automaton::{
    self, AutomatonContext, ChildAutomaton, ChildMatcher, KindConstraint, PatternId, State,
};
use super::facts::{GrammarFacts, NodeRealizer};
use super::state_set::StateSet;

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

type SatKey = (PatternId, NodeRealizer);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ThreadKey {
    pattern: PatternId,
    hidden_var: VarId,
    state: State,
    /// Part of the key: the same frontier entered under different labels admits
    /// different `field:` matchers, so the memo must keep them apart.
    inherited_field: Option<NodeFieldId>,
}

struct ProductionThread<'f, 'q, 'g> {
    frozen: &'f Frozen<'q>,
    pattern: PatternId,
    inherited_field: Option<NodeFieldId>,
    gaps: &'g mut [GapClass],
}

impl<'f, 'q, 'g> ProductionThread<'f, 'q, 'g> {
    fn new(
        frozen: &'f Frozen<'q>,
        pattern: PatternId,
        inherited_field: Option<NodeFieldId>,
        gaps: &'g mut [GapClass],
    ) -> Self {
        Self {
            frozen,
            pattern,
            inherited_field,
            gaps,
        }
    }

    fn automaton(&self) -> &ChildAutomaton {
        self.frozen.automaton(self.pattern)
    }

    fn hidden_key(
        &self,
        hidden_var: VarId,
        state: State,
        inherited_field: Option<NodeFieldId>,
    ) -> ThreadKey {
        ThreadKey {
            pattern: self.pattern,
            hidden_var,
            state,
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
    ctx: AutomatonContext<'a>,
    anchor_semantics: AnchorSemantics<'a>,
    automata: Vec<ChildAutomaton>,
    table: automaton::PatternTable,
    facts: Arc<GrammarFacts>,
    relax_anchors: bool,
    /// Native-recursion ceiling for automaton construction while inlining references.
    automaton_max_depth: u32,
}

impl<'a> Frozen<'a> {
    fn automaton(&self, p: PatternId) -> &ChildAutomaton {
        &self.automata[p.index()]
    }

    fn variable(&self, v: VarId) -> &SkeletonVariable {
        self.ctx
            .grammar
            .structure()
            .variable(v)
            .expect("a VarId from the grammar structure always resolves")
    }

    fn realizers_of(&self, kind: NodeKindId) -> &[NodeRealizer] {
        self.facts.realizers_of(kind)
    }

    fn build_automaton(
        &mut self,
        node: &Located<NamedNodePattern>,
        remaining_budget: u64,
    ) -> ChildAutomaton {
        automaton::build(
            node,
            self.ctx,
            &mut self.table,
            &self.anchor_semantics,
            self.relax_anchors,
            self.automaton_max_depth,
            remaining_budget,
        )
    }

    /// Whether a child-position kind constraint admits visible grammar kind `k`.
    /// Query supertypes are rejected before this pass runs, and grammar supertype
    /// steps are classified as hidden frontiers before matching.
    fn kind_ok(&self, constraint: KindConstraint, k: NodeKindId) -> bool {
        let grammar = self.ctx.grammar;
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
        self.facts.admits_any_extra(self.ctx.grammar, constraint)
    }

    /// The concrete named kinds a wildcard parent could be: every named, non-supertype
    /// kind the grammar can surface. A wildcard with children is satisfiable iff one of
    /// these takes those children — a token can never be a parent, so it is excluded.
    fn parent_candidate_kinds(&self) -> &[NodeKindId] {
        self.facts.parent_candidate_kinds()
    }

    /// Whether any extra kind admitted by `constraint` satisfies `predicate`.
    fn any_extra_admitted_by(
        &self,
        constraint: KindConstraint,
        predicate: impl FnMut(NodeKindId) -> bool,
    ) -> bool {
        self.facts
            .any_extra_admitted_by(self.ctx.grammar, constraint, predicate)
    }

    /// The kinds that can be the first (or last) child of a node of `kind`: the
    /// leading (trailing) visible step of each production, descending through hidden
    /// frontiers. Best-effort for diagnostics — it may under-list past a nullable
    /// hidden rule, so messages phrase it positively ("begins with …"), never "never".
    fn edge_child_kinds(&self, kind: NodeKindId, edge: Edge) -> Vec<NodeKindId> {
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        for &realizer in self.realizers_of(kind) {
            if let NodeRealizer::Var(var) = realizer {
                self.edge_kinds_of_var(var, edge, &mut out, &mut visited);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn edge_kinds_of_var(
        &self,
        var: VarId,
        edge: Edge,
        out: &mut Vec<NodeKindId>,
        visited: &mut HashSet<VarId>,
    ) {
        if !visited.insert(var) {
            return;
        }
        for production in &self.variable(var).productions {
            match edge {
                Edge::First => self.edge_kinds_of_steps(production.iter(), edge, out, visited),
                Edge::Last => self.edge_kinds_of_steps(production.iter().rev(), edge, out, visited),
            }
        }
    }

    fn edge_kinds_of_steps<'s>(
        &self,
        steps: impl Iterator<Item = &'s SkeletonStep>,
        edge: Edge,
        out: &mut Vec<NodeKindId>,
        visited: &mut HashSet<VarId>,
    ) {
        for step in steps {
            match StepClass::of(step, self.ctx.grammar) {
                StepClass::Visible(visible) => {
                    out.push(visible.kind);
                    break;
                }
                StepClass::HiddenSubtree(hidden) => {
                    self.edge_kinds_of_var(hidden.var, edge, out, visited);
                    break;
                }
                // A hidden token surfaces nothing — the edge child is further along.
                StepClass::HiddenLeaf => {}
            }
        }
    }
}

/// Which end of a node's child sequence [`Frozen::edge_child_kinds`] asks about.
#[derive(Clone, Copy)]
enum Edge {
    First,
    Last,
}

/// Default ceiling on satisfiability work before the query is declared too complex.
/// Charged for automaton state allocation and in the two quadratic solve loops
/// (`closure` and a `Visible` `thread_step`) for state visits and pattern-edge scans,
/// so it caps the dominant costs. The widest real snapshot settles in a few thousand
/// work units, leaving roughly three orders of magnitude of headroom, while a child list
/// past about a thousand wide trips it in a fraction of a second rather than running
/// for tens. Tunable per query via
/// [`QueryBuilder::with_satisfiability_work_budget`](crate::QueryBuilder::with_satisfiability_work_budget)
/// for the rare case that legitimately needs a wider one.
pub const DEFAULT_SATISFIABILITY_WORK_BUDGET: u64 = 2_000_000;

/// The mutable fixed-point state: memo tables, reverse dependencies, and the worklist.
#[derive(Default)]
struct Solve {
    sat: HashMap<SatKey, bool>,
    thread: HashMap<ThreadKey, StateSet>,
    /// `dependents[k]` are the keys that read `k` — re-queued when `k` changes.
    dependents: HashMap<Key, IndexSet<Key>>,
    /// The key currently being recomputed, so reads attribute to it.
    current: Option<Key>,
    queue: VecDeque<Key>,
    queued: HashSet<Key>,
    /// Work units charged so far, the ceiling they may reach, and whether they
    /// crossed it. Once `exhausted`, callers stop trusting memo values and reject
    /// the query as too complex instead.
    work_used: u64,
    budget: u64,
    exhausted: bool,
}

pub(super) struct SatisfiabilitySolver<'a> {
    frozen: Frozen<'a>,
    solve: Solve,
}

impl<'a> SatisfiabilitySolver<'a> {
    pub(super) fn checking(ctx: AutomatonContext<'a>, limits: SatisfiabilityLimits) -> Self {
        let relax_anchors = false;
        Self::with_anchor_relaxation(
            ctx,
            relax_anchors,
            limits,
            Arc::new(GrammarFacts::from_grammar(ctx.grammar)),
        )
    }

    pub(super) fn relaxing_anchors(&self, work_budget: u64) -> Self {
        let limits = SatisfiabilityLimits {
            automaton_max_depth: self.frozen.automaton_max_depth,
            work_budget,
        };
        let relax_anchors = true;
        Self::with_anchor_relaxation(
            self.frozen.ctx,
            relax_anchors,
            limits,
            Arc::clone(&self.frozen.facts),
        )
    }

    pub(super) fn remaining_budget(&self) -> u64 {
        self.solve.remaining_budget()
    }

    fn with_anchor_relaxation(
        ctx: AutomatonContext<'a>,
        relax_anchors: bool,
        limits: SatisfiabilityLimits,
        facts: Arc<GrammarFacts>,
    ) -> Self {
        Self {
            frozen: Frozen {
                ctx,
                anchor_semantics: AnchorSemantics::new(ctx.interner, ctx.definitions),
                automata: Vec::new(),
                table: automaton::PatternTable::default(),
                facts,
                relax_anchors,
                automaton_max_depth: limits.automaton_max_depth,
            },
            solve: Solve {
                budget: limits.work_budget,
                ..Solve::default()
            },
        }
    }

    /// Whether some realizer of grammar kind `kind` can realize `node`'s child structure.
    /// Errs toward `true` (accept) whenever the question cannot be decided, so a
    /// rejection is always sound.
    pub(super) fn satisfiable(
        &mut self,
        node: &Located<NamedNodePattern>,
        kind: NodeKindId,
    ) -> bool {
        // Already over budget on an earlier node: accept here too and let the pass
        // reject the whole query, rather than start a fresh solve we cannot finish.
        if self.solve.exhausted {
            return true;
        }
        let p = self.frozen.table.intern(node.clone());
        self.build_pending();

        let realizer_count = self.frozen.realizers_of(kind).len();
        if realizer_count == 0 {
            // No realizer of this kind — we cannot reason about it, so accept.
            return true;
        }
        for index in 0..realizer_count {
            let realizer = self.frozen.realizers_of(kind)[index];
            self.solve.seed(Key::Sat((p, realizer)));
        }
        self.run();
        // A solve that ran out of budget left its memo partly converged; its `false`s
        // are not sound rejections, so accept and defer to the too-complex check.
        if self.solve.exhausted {
            return true;
        }
        for index in 0..realizer_count {
            let realizer = self.frozen.realizers_of(kind)[index];
            if self.solve.sat_value((p, realizer)) {
                return true;
            }
        }
        false
    }

    /// Whether some named kind the grammar surfaces can have `node`'s children — the
    /// satisfiability question for a wildcard parent `(_ …)`, which fixes no kind of its
    /// own. Accept on the first candidate that works; only an impossible wildcard pays
    /// for ruling every candidate out, and only a wildcard with child-structure
    /// constraints reaches here at all.
    pub(super) fn wildcard_satisfiable(&mut self, node: &Located<NamedNodePattern>) -> bool {
        for index in 0..self.frozen.parent_candidate_kinds().len() {
            let kind = self.frozen.parent_candidate_kinds()[index];
            if self.satisfiable(node, kind) {
                return true;
            }
        }
        false
    }

    /// Build every automaton interned so far (transitively reaching all child
    /// patterns), so the solve phase reads a frozen automaton set.
    fn build_pending(&mut self) {
        while self.frozen.automata.len() < self.frozen.table.len() {
            let remaining_budget = self.solve.remaining_budget();
            if remaining_budget == 0 {
                self.solve.exhausted = true;
                break;
            }

            let index = self.frozen.automata.len();
            let node = self.frozen.table.node_at(index).clone();
            let automaton = self.frozen.build_automaton(&node, remaining_budget);
            self.solve.spend(automaton.state_count() as u64);
            self.frozen.automata.push(automaton);
        }
    }

    fn run(&mut self) {
        while let Some(key) = self.solve.dequeue() {
            // Budget spent: stop draining. The verdict is no longer trusted — the
            // caller accepts and the pass rejects the query as too complex.
            if self.solve.exhausted {
                break;
            }
            if self.solve.recompute(&self.frozen, key) {
                self.solve.requeue_dependents(key);
            }
        }
    }

    pub(super) fn context(&self) -> AutomatonContext<'a> {
        self.frozen.ctx
    }

    /// Whether a resource ceiling tripped: an automaton bailed on construction (state
    /// cap or recursion depth), or the solve ran past its work budget. Either way the
    /// query is rejected as too complex, rather than judged on an automaton we declined
    /// to finish or a fixed point we declined to reach.
    pub(super) fn is_too_complex(&self) -> bool {
        self.solve.exhausted || self.frozen.automata.iter().any(|a| a.is_too_complex())
    }

    /// The kinds a node of `kind` can begin with — for a leading-anchor diagnostic.
    pub(super) fn first_child_kinds(&self, kind: NodeKindId) -> Vec<NodeKindId> {
        self.frozen.edge_child_kinds(kind, Edge::First)
    }

    /// The kinds a node of `kind` can end with — for a trailing-anchor diagnostic.
    pub(super) fn last_child_kinds(&self, kind: NodeKindId) -> Vec<NodeKindId> {
        self.frozen.edge_child_kinds(kind, Edge::Last)
    }
}

impl Solve {
    fn remaining_budget(&self) -> u64 {
        self.budget.saturating_sub(self.work_used)
    }

    fn spend(&mut self, work_units: u64) {
        self.work_used = self.work_used.saturating_add(work_units);
        if self.work_used > self.budget {
            self.exhausted = true;
        }
    }

    fn sat_value(&self, key: SatKey) -> bool {
        self.sat.get(&key).copied().unwrap_or_else(|| {
            panic!("satisfiability solver read key {key:?} before `Solve::seed` initialized it")
        })
    }

    /// Charge one unit of solver work, returning whether the caller may continue.
    /// Called in the hot loops so total work — not any single key — is what the
    /// bound governs.
    fn charge(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        self.work_used = self.work_used.saturating_add(1);
        if self.work_used > self.budget {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn seed(&mut self, key: Key) {
        let fresh = match key {
            Key::Sat(k) => !self.sat.contains_key(&k) && self.sat.insert(k, false).is_none(),
            Key::Thread(k) => {
                !self.thread.contains_key(&k)
                    && self.thread.insert(k, StateSet::default()).is_none()
            }
        };
        if fresh {
            self.enqueue(key);
        }
    }

    fn enqueue(&mut self, key: Key) {
        if self.queued.insert(key) {
            self.queue.push_back(key);
        }
    }

    /// LIFO: a freshly demanded key is recomputed before the key that demanded it
    /// resumes, so a dependency chain settles deepest-first and each fact is revisited
    /// far fewer times than a breadth-first order would force.
    fn dequeue(&mut self) -> Option<Key> {
        let key = self.queue.pop_back()?;
        self.queued.remove(&key);
        Some(key)
    }

    fn requeue_dependents(&mut self, key: Key) {
        let Some(dependents) = self.dependents.get(&key) else {
            return;
        };
        let dependents: Vec<Key> = dependents.iter().copied().collect();
        for dependent in dependents {
            self.enqueue(dependent);
        }
    }

    fn record_read(&mut self, read: Key) {
        if let Some(current) = self.current {
            self.dependents.entry(read).or_default().insert(current);
        }
    }

    fn get_sat(&mut self, key: SatKey) -> bool {
        self.record_read(Key::Sat(key));
        self.seed(Key::Sat(key));
        self.sat_value(key)
    }

    fn get_thread(&mut self, key: ThreadKey) -> StateSet {
        self.record_read(Key::Thread(key));
        self.seed(Key::Thread(key));
        self.thread.get(&key).cloned().unwrap_or_else(|| {
            panic!("satisfiability solver read thread {key:?} before `Solve::seed` initialized it")
        })
    }

    fn recompute(&mut self, frozen: &Frozen, key: Key) -> bool {
        let previous = self.current.replace(key);
        let changed = match key {
            Key::Sat(k) => {
                let computed = self.compute_sat(frozen, k);
                self.sat_value(k) != computed && {
                    self.sat.insert(k, computed);
                    true
                }
            }
            Key::Thread(k) => {
                let computed = self.compute_thread(frozen, k);
                let changed = self.thread.get(&k).unwrap_or_else(|| {
                    panic!(
                        "satisfiability solver dequeued thread {k:?}, but its seeded state was \
                             missing during recomputation"
                    )
                }) != &computed;
                if changed {
                    self.thread.insert(k, computed);
                }
                changed
            }
        };
        self.current = previous;
        changed
    }

    fn compute_sat(&mut self, frozen: &Frozen, (p, realizer): SatKey) -> bool {
        let automaton = frozen.automaton(p);
        // A construction that bailed on a resource ceiling left a half-built automaton; its
        // verdicts are not sound, so accept and let the pass reject the query as too complex.
        if automaton.is_too_complex() {
            return true;
        }
        let start = StateSet::singleton(automaton.start());
        let accept = automaton.accept();
        match realizer {
            // A token has no children, not even extras: it realizes `p` only if `p`
            // accepts the empty child sequence.
            NodeRealizer::Leaf => eps_closure(automaton, &start).contains(accept),
            // Some production of the variable threads `A_p` from start to accept. These
            // are the node's own children, so no field is inherited from above.
            NodeRealizer::Var(v) => {
                let production_count = frozen.variable(v).productions.len();
                let mut gaps = gap_scratch(frozen, p);
                let mut thread = ProductionThread::new(frozen, p, None, &mut gaps);
                (0..production_count).any(|i| {
                    let production = &frozen.variable(v).productions[i];
                    self.thread_production(&mut thread, production, &start)
                        .contains(accept)
                })
            }
        }
    }

    fn compute_thread(&mut self, frozen: &Frozen, key: ThreadKey) -> StateSet {
        let start = StateSet::singleton(key.state);
        let mut reached = StateSet::default();
        let mut gaps = gap_scratch(frozen, key.pattern);
        let mut thread = ProductionThread::new(frozen, key.pattern, key.inherited_field, &mut gaps);
        let production_count = frozen.variable(key.hidden_var).productions.len();
        for i in 0..production_count {
            let production = &frozen.variable(key.hidden_var).productions[i];
            let states = self.thread_production(&mut thread, production, &start);
            reached.union_with(&states);
        }
        reached
    }

    /// Thread one production's steps, left to right, through `A_p` from `start`,
    /// returning the reachable state set. `thread.inherited_field` is the field a hidden ancestor
    /// step pushed onto this frontier — `None` for a node's own children.
    fn thread_production(
        &mut self,
        thread: &mut ProductionThread<'_, '_, '_>,
        production: &[SkeletonStep],
        start: &StateSet,
    ) -> StateSet {
        let mut current = self.thread_closure(thread, start);
        for step in production {
            // A dead frontier stays dead; the rest of the production cannot revive it.
            if self.exhausted || current.is_empty() {
                break;
            }
            current = self.thread_step(thread, &current, step);
            if self.exhausted {
                break;
            }
            current = self.thread_closure(thread, &current);
        }
        current
    }

    fn thread_step(
        &mut self,
        thread: &mut ProductionThread<'_, '_, '_>,
        current: &StateSet,
        step: &SkeletonStep,
    ) -> StateSet {
        match StepClass::of(step, thread.frozen.ctx.grammar) {
            // A real child of kind `kind`. Each current state either skips it through a
            // gap self-loop, or consumes it through an edge whose kind and field both
            // admit the step. The step's own label wins over an inherited one.
            StepClass::Visible(visible) => self.thread_visible_step(thread, current, visible),
            // Splice the hidden variable's visible frontier in, pushing down the label it
            // inherits: this step's own field if it has one, otherwise the one already
            // inherited (a plain supertype link never relabels what it carries).
            StepClass::HiddenSubtree(hidden) => self.thread_hidden_subtree(thread, current, hidden),
            // A hidden token surfaces nothing and consumes nothing.
            StepClass::HiddenLeaf => current.clone(),
        }
    }

    fn thread_hidden_subtree(
        &mut self,
        thread: &ProductionThread<'_, '_, '_>,
        current: &StateSet,
        hidden: HiddenStep,
    ) -> StateSet {
        let pushed = hidden.pushed_field(thread.inherited_field);
        let mut next = StateSet::default();
        for q in current.iter() {
            if !self.charge() {
                break;
            }
            let reached = self.get_thread(thread.hidden_key(hidden.var, q, pushed));
            next.union_with(&reached);
        }
        next
    }

    fn thread_visible_step(
        &mut self,
        thread: &mut ProductionThread<'_, '_, '_>,
        current: &StateSet,
        visible: VisibleStep,
    ) -> StateSet {
        let automaton = thread.automaton();
        let effective = visible.effective_field(thread.inherited_field);
        // The query asserts this field absent (`-field`); a production binding it
        // gives the node a forbidden child, so this whole path is dead — it can be
        // neither consumed nor skipped past.
        if automaton.negates(effective) {
            return StateSet::default();
        }
        let node_class = thread.frozen.ctx.grammar.node_class(visible.kind);
        let mut next = StateSet::default();
        for q in current.iter() {
            if !self.charge() {
                break;
            }
            // The state's *effective* gap (tightest erasure path that reaches it),
            // so an exact anchor erased into this position still forbids the skip.
            if thread.gaps[q as usize].admits(node_class) {
                next.insert(q);
            }
            for (matcher, to) in automaton.pattern_edges(q) {
                if !self.charge() {
                    break;
                }
                if thread.frozen.kind_ok(matcher.kind, visible.kind)
                    && field_ok(matcher.field, effective)
                    && self.child_sat(matcher, visible.realizer)
                {
                    next.insert(*to);
                }
            }
        }
        next
    }

    fn thread_closure(
        &mut self,
        thread: &mut ProductionThread<'_, '_, '_>,
        set: &StateSet,
    ) -> StateSet {
        let frozen = thread.frozen;
        let automaton = frozen.automaton(thread.pattern);
        self.closure(frozen, automaton, set, thread.gaps)
    }

    /// Whether the matched child's own structure is realized by `realizer` — trivially
    /// true when the child is childless.
    fn child_sat(&mut self, matcher: &ChildMatcher, realizer: NodeRealizer) -> bool {
        match matcher.nested_pattern {
            None => true,
            Some(nested_pattern) => self.get_sat((nested_pattern, realizer)),
        }
    }

    /// Epsilon closure plus optional extra consumption: a query child matching an
    /// extra kind (a `(comment)`) may advance the automaton without a production step,
    /// since the parser may insert an extra anywhere. Extras are optional, so the
    /// closure only grows the reachable set.
    fn closure(
        &mut self,
        frozen: &Frozen,
        automaton: &ChildAutomaton,
        set: &StateSet,
        gaps: &mut [GapClass],
    ) -> StateSet {
        // Reachability over epsilon edges plus extra-consumable pattern edges (a query
        // child matching an inserted extra advances without a production step). Alongside
        // membership, carry each state's effective skip gap: tightest *along* a path
        // (every erased step bounds what may be skipped after it), loosest *across* paths
        // (a skip is open if any path opens it). So an exact anchor survives erasure — a
        // state reached only by erasing optionals under `.!` cannot then skip what the
        // anchor forbids — while a state also reachable by consuming keeps its own gap.
        let mut result = set.clone();
        let mut stack: Vec<State> = Vec::new();
        for q in result.iter() {
            gaps[q as usize] = automaton.gap(q);
            stack.push(q);
        }
        while let Some(q) = stack.pop() {
            if !self.charge() {
                break;
            }
            let gq = gaps[q as usize];
            for &to in automaton.eps_edges(q) {
                if !self.charge() {
                    break;
                }
                let candidate = gq.tighten(automaton.gap(to));
                if relax_into(gaps, &mut result, to, candidate) {
                    stack.push(to);
                }
            }
            for (matcher, to) in automaton.pattern_edges(q) {
                if !self.charge() {
                    break;
                }
                if self.can_consume_extra(frozen, matcher) {
                    let candidate = gq.tighten(automaton.gap(*to));
                    if relax_into(gaps, &mut result, *to, candidate) {
                        stack.push(*to);
                    }
                }
            }
        }
        result
    }

    /// Whether a query child matching an extra kind may advance the automaton without a
    /// production step — the satisfiability mirror of `check.rs`'s extras rescue, and the
    /// same tolerated over-acceptance: an extra is consumable in *any* gap, including
    /// lexically sealed nodes (`(string (comment))`). Proving a gap sealed needs lexer-level
    /// longest-match reasoning our model lacks (not the `IMMEDIATE_TOKEN` fact it looks
    /// like), so we admit and stay sound — extra consumption only grows the reachable set.
    fn can_consume_extra(&mut self, frozen: &Frozen, matcher: &ChildMatcher) -> bool {
        // Extras are inserted unfielded, so a `field:` matcher can never be one — letting
        // it "consume" an extra here would skip its field constraint.
        if matcher.field.is_some() {
            return false;
        }
        let Some(nested_pattern) = matcher.nested_pattern else {
            // Childless matcher: it consumes an extra iff it admits any extra kind. No
            // subtree to realize, so answer in O(1) without building the admitted list —
            // the hot path on wide wildcard child lists.
            return frozen.admits_any_extra(matcher.kind);
        };
        frozen.any_extra_admitted_by(matcher.kind, |extra| {
            frozen
                .realizers_of(extra)
                .iter()
                .copied()
                .any(|realizer| self.get_sat((nested_pattern, realizer)))
        })
    }
}

/// Add `to` to the closure under effective gap `candidate`, returning whether it must be
/// (re)visited — newly reached, or reached by a more permissive path that loosened its
/// gap (so its successors must see the wider skip permission).
fn relax_into(
    gaps: &mut [GapClass],
    result: &mut StateSet,
    to: State,
    candidate: GapClass,
) -> bool {
    if result.insert(to) {
        gaps[to as usize] = candidate;
        return true;
    }
    let loosened = gaps[to as usize].loosen(candidate);
    if loosened != gaps[to as usize] {
        gaps[to as usize] = loosened;
        return true;
    }
    false
}

/// Pure epsilon closure: no memo reads, so it needs neither the solver nor `Frozen`.
fn eps_closure(automaton: &ChildAutomaton, set: &StateSet) -> StateSet {
    let mut result = set.clone();
    let mut stack: Vec<State> = result.iter().collect();
    while let Some(q) = stack.pop() {
        for &to in automaton.eps_edges(q) {
            if result.insert(to) {
                stack.push(to);
            }
        }
    }
    result
}

/// Effective skip gap per state, recomputed by each `closure` and read by the next
/// `thread_step`. States outside the live frontier are never read, so stale entries
/// between productions do not matter.
fn gap_scratch(frozen: &Frozen, p: PatternId) -> Vec<GapClass> {
    vec![GapClass::Any; frozen.automaton(p).state_count()]
}
