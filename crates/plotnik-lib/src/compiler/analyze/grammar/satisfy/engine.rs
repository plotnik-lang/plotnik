//! The `SAT`/`THREAD` least-fixed-point.
//!
//! Two mutually-recursive judgments over finite, monotone domains:
//!
//! - `SAT(p, producer)` — a node built from grammar `producer` (a token `Leaf`, or a
//!   non-terminal `Var`) can realize the child structure query node `p` demands.
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
//! width. So the solve carries a step budget; once the per-state work exceeds it the
//! solve gives up, every pending verdict reads as *accept* (the sound default), and
//! the pass rejects the whole query as too complex rather than spend unbounded time.
//! The budget bounds work, not wall-clock, so the cut-off is the same on every
//! machine — a slow host merely takes proportionally longer to reach it.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::parse::ast::NodePattern;
use crate::core::grammar::{Grammar, SkeletonStep, SkeletonVariable, VarId};
use crate::core::{NodeFieldId, NodeKindId};

use super::automaton::{
    self, AutomatonContext, ChildAutomaton, ChildMatcher, KindConstraint, PatternId, State,
};
use super::state_set::StateSet;

/// What builds a grammar node: a token (no children) or a non-terminal variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Producer {
    Leaf,
    Var(VarId),
}

impl Producer {
    /// The producer a visible step descends into: its own variable, or `Leaf` for a
    /// token. Keying `SAT` by the step's own `body` — never "any variable producing
    /// this kind" — is what makes nesting alias-correct.
    fn of_step(step: &SkeletonStep) -> Self {
        step.target.body.map(Producer::Var).unwrap_or(Producer::Leaf)
    }
}

/// How a production step participates in threading.
enum StepClass {
    /// A real child surfacing under `kind`, built by `producer`, bound to `field` when
    /// the grammar labels it. A label on this step overrides any pushed down from a
    /// hidden ancestor (the innermost label is the one the runtime attaches).
    Visible {
        kind: NodeKindId,
        field: Option<NodeFieldId>,
        producer: Producer,
    },
    /// A child spliced in without an id of its own: thread through its frontier.
    HiddenSubtree(VarId),
    /// A hidden token: present in the production, absent from the tree.
    HiddenLeaf,
}

/// Whether a matcher demanding field `want` accepts a child whose runtime label is
/// `have`. A bare matcher (`want` is `None`) imposes no field constraint.
fn field_ok(want: Option<NodeFieldId>, have: Option<NodeFieldId>) -> bool {
    want.is_none() || want == have
}

type SatKey = (PatternId, Producer);
/// `(query node, hidden variable, automaton state, inherited field)`. The inherited
/// field is part of the key: the same frontier entered under different labels admits
/// different `field:` matchers, so the memo must keep them apart.
type ThreadKey = (PatternId, VarId, State, Option<NodeFieldId>);

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
    automata: Vec<ChildAutomaton>,
    table: automaton::PatternTable,
    /// Kind → the producers that can build a node of that kind: the variable named
    /// for it, plus every aliased step occurrence surfacing it.
    producers: HashMap<NodeKindId, Vec<Producer>>,
    /// Visible extra kinds (comments), and the named subset, for extra-consumption.
    extras: Vec<NodeKindId>,
    named_extras: Vec<NodeKindId>,
    relax: bool,
    /// Structural-depth ceiling for automaton construction — the parser's `max_depth`.
    max_depth: u32,
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

    fn producers_of(&self, kind: NodeKindId) -> &[Producer] {
        self.producers.get(&kind).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Classify a step for threading. A supertype is erased in the tree — tree-sitter
    /// never emits a node of the supertype's kind, only one of its subtypes — so a
    /// step surfacing a supertype is threaded through its body, not matched as a node.
    /// Keying the descent off the step's own `body` is what keeps aliased nodes
    /// structurally distinct from their namesakes.
    fn classify(&self, step: &SkeletonStep) -> StepClass {
        match (step.target.id, step.target.body) {
            (Some(id), body) if self.ctx.grammar.is_supertype(id) => match body {
                Some(var) => StepClass::HiddenSubtree(var),
                None => StepClass::HiddenLeaf,
            },
            (Some(kind), _) => StepClass::Visible {
                kind,
                field: step.field,
                producer: Producer::of_step(step),
            },
            (None, Some(var)) => StepClass::HiddenSubtree(var),
            (None, None) => StepClass::HiddenLeaf,
        }
    }

    /// Whether a child-position kind constraint admits grammar kind `k`. Query
    /// supertypes are rejected before this pass runs, so the only supertype case is a
    /// *grammar* step that surfaces as a supertype: a concrete query kind matches it
    /// if it is one of the supertype's subtypes.
    fn kind_ok(&self, constraint: KindConstraint, k: NodeKindId) -> bool {
        let grammar = self.ctx.grammar;
        match constraint {
            KindConstraint::Exact(id) => {
                id == k
                    || (grammar.is_supertype(k) && grammar.collect_subtypes(k).contains(&id))
            }
            KindConstraint::AnyNamed => !grammar.is_anonymous_node(k),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => true,
        }
    }

    /// Whether the matcher's kind admits at least one extra kind. The childless fast
    /// path of [`Satisfier::can_consume_extra`] — answered without materializing the
    /// admitted-extra list, which matters on wide wildcard child lists.
    fn admits_any_extra(&self, constraint: KindConstraint) -> bool {
        match constraint {
            KindConstraint::Exact(id) => self.ctx.grammar.is_extra(id),
            KindConstraint::AnyNamed => !self.named_extras.is_empty(),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => !self.extras.is_empty(),
        }
    }

    /// The concrete named kinds a wildcard parent could be: every named, non-supertype
    /// kind the grammar can build. A wildcard with children is satisfiable iff one of
    /// these takes those children — a token can never be a parent, so it is excluded.
    fn parent_candidate_kinds(&self) -> Vec<NodeKindId> {
        self.producers
            .keys()
            .copied()
            .filter(|&k| !self.ctx.grammar.is_anonymous_node(k) && !self.ctx.grammar.is_supertype(k))
            .collect()
    }

    /// Extra kinds a child matcher could consume. Only kinds the matcher admits — a
    /// `(comment)` admits the `comment` extra, a `(_)` any named extra, `_` any extra.
    fn extras_admitted_by(&self, constraint: KindConstraint) -> Vec<NodeKindId> {
        match constraint {
            KindConstraint::Exact(id) => {
                if self.ctx.grammar.is_extra(id) {
                    vec![id]
                } else {
                    Vec::new()
                }
            }
            KindConstraint::AnyNamed => self.named_extras.clone(),
            KindConstraint::AnyNode | KindConstraint::Unconstrained => self.extras.clone(),
        }
    }

    /// The kinds that can be the first (or last) child of a node of `kind`: the
    /// leading (trailing) visible step of each production, descending through hidden
    /// frontiers. Best-effort for diagnostics — it may under-list past a nullable
    /// hidden rule, so messages phrase it positively ("begins with …"), never "never".
    fn edge_child_kinds(&self, kind: NodeKindId, edge: Edge) -> Vec<NodeKindId> {
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        for &producer in self.producers_of(kind) {
            if let Producer::Var(var) = producer {
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
            let mut steps: Vec<&SkeletonStep> = production.iter().collect();
            if matches!(edge, Edge::Last) {
                steps.reverse();
            }
            for step in steps {
                match self.classify(step) {
                    StepClass::Visible { kind, .. } => {
                        out.push(kind);
                        break;
                    }
                    StepClass::HiddenSubtree(h) => {
                        self.edge_kinds_of_var(h, edge, out, visited);
                        break;
                    }
                    // A hidden token surfaces nothing — the edge child is further along.
                    StepClass::HiddenLeaf => {}
                }
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

/// Default ceiling on solve work before the query is declared too complex. Charged
/// once per visited state in the two quadratic inner loops (`closure` and a `Visible`
/// `thread_step`), so it caps the dominant cost. The widest real fixture settles in a
/// few thousand steps, so this leaves roughly three orders of magnitude of headroom,
/// while a child list past about a thousand wide — quadratic at ~2n² steps — trips it
/// in a fraction of a second rather than running for tens. Tunable per query via
/// [`QueryBuilder::with_satisfy_step_budget`](crate::QueryBuilder::with_satisfy_step_budget)
/// for the rare case that legitimately needs a wider one.
pub const DEFAULT_SATISFY_STEP_BUDGET: u64 = 2_000_000;

/// The mutable fixed-point state: memo tables, reverse dependencies, and the worklist.
#[derive(Default)]
struct Solve {
    sat: HashMap<SatKey, bool>,
    thread: HashMap<ThreadKey, StateSet>,
    /// `dependents[k]` are the keys that read `k` — re-queued when `k` changes.
    dependents: HashMap<Key, HashSet<Key>>,
    /// The key currently being recomputed, so reads attribute to it.
    current: Option<Key>,
    queue: VecDeque<Key>,
    queued: HashSet<Key>,
    /// State-visits charged so far, the ceiling they may reach, and whether they
    /// crossed it. Once `exhausted`, the solve stops doing work and every verdict
    /// reads as accept.
    steps: u64,
    budget: u64,
    exhausted: bool,
}

pub(super) struct Satisfier<'a> {
    frozen: Frozen<'a>,
    solve: Solve,
}

impl<'a> Satisfier<'a> {
    pub(super) fn new(
        ctx: AutomatonContext<'a>,
        relax: bool,
        max_depth: u32,
        step_budget: u64,
    ) -> Self {
        let (extras, named_extras) = extra_kinds(ctx.grammar);
        Self {
            frozen: Frozen {
                ctx,
                automata: Vec::new(),
                table: automaton::PatternTable::default(),
                producers: build_producers(ctx.grammar),
                extras,
                named_extras,
                relax,
                max_depth,
            },
            solve: Solve {
                budget: step_budget,
                ..Solve::default()
            },
        }
    }

    /// Whether the grammar can build a node of `kind` whose children realize `node`.
    /// Errs toward `true` (accept) whenever the question cannot be decided, so a
    /// rejection is always sound.
    pub(super) fn satisfiable(&mut self, node: &Located<NodePattern>, kind: NodeKindId) -> bool {
        // Already over budget on an earlier node: accept here too and let the pass
        // reject the whole query, rather than start a fresh solve we cannot finish.
        if self.solve.exhausted {
            return true;
        }
        let p = self.frozen.table.intern(node.clone());
        self.build_pending();

        let producers = self.frozen.producers_of(kind).to_vec();
        if producers.is_empty() {
            // No producer of this kind — we cannot reason about it, so accept.
            return true;
        }
        for &producer in &producers {
            self.solve.seed(Key::Sat((p, producer)));
        }
        self.run();
        // A solve that ran out of budget left its memo partly converged; its `false`s
        // are not sound rejections, so accept and defer to the too-complex check.
        if self.solve.exhausted {
            return true;
        }
        producers
            .iter()
            .any(|&producer| self.solve.sat_value((p, producer)))
    }

    /// Whether some named node the grammar builds can have `node`'s children — the
    /// satisfiability question for a wildcard parent `(_ …)`, which fixes no kind of its
    /// own. Accept on the first candidate that works; only an impossible wildcard pays
    /// for ruling every candidate out, and only a wildcard *with* a child list reaches
    /// here at all.
    pub(super) fn wildcard_satisfiable(&mut self, node: &Located<NodePattern>) -> bool {
        let candidates = self.frozen.parent_candidate_kinds();
        candidates.iter().any(|&kind| self.satisfiable(node, kind))
    }

    /// Build every automaton interned so far (transitively reaching all child
    /// patterns), so the solve phase reads a frozen automaton set.
    fn build_pending(&mut self) {
        while self.frozen.automata.len() < self.frozen.table.len() {
            let index = self.frozen.automata.len();
            let node = self.frozen.table.node_at(index).clone();
            let automaton = automaton::build(
                &node,
                self.frozen.ctx,
                &mut self.frozen.table,
                self.frozen.relax,
                self.frozen.max_depth,
            );
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

    /// The structural-depth ceiling threaded into construction — so a secondary probe
    /// (the relaxed-anchor diagnostic) bounds its own build the same way.
    pub(super) fn max_depth(&self) -> u32 {
        self.frozen.max_depth
    }

    /// The solve's work ceiling — so a secondary probe (the relaxed-anchor diagnostic)
    /// runs under the same budget as the primary solve.
    pub(super) fn step_budget(&self) -> u64 {
        self.solve.budget
    }

    /// Whether a resource ceiling tripped: an automaton bailed on construction (state
    /// cap or recursion depth), or the solve ran past its step budget. Either way the
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
    fn sat_value(&self, key: SatKey) -> bool {
        self.sat.get(&key).copied().unwrap_or(false)
    }

    /// Charge one unit of state-visiting work, latching `exhausted` once the budget is
    /// spent. Called in the hot per-state loops so total work — not any single key — is
    /// what the bound governs.
    fn charge(&mut self) {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.budget {
            self.exhausted = true;
        }
    }

    fn seed(&mut self, key: Key) {
        let fresh = match key {
            Key::Sat(k) => !self.sat.contains_key(&k) && self.sat.insert(k, false).is_none(),
            Key::Thread(k) => {
                !self.thread.contains_key(&k) && self.thread.insert(k, StateSet::default()).is_none()
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
        self.thread.get(&key).cloned().unwrap_or_default()
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
                let changed = self.thread.get(&k) != Some(&computed);
                if changed {
                    self.thread.insert(k, computed);
                }
                changed
            }
        };
        self.current = previous;
        changed
    }

    fn compute_sat(&mut self, frozen: &Frozen, (p, producer): SatKey) -> bool {
        let automaton = frozen.automaton(p);
        // A construction that bailed on a resource ceiling left a half-built automaton; its
        // verdicts are not sound, so accept and let the pass reject the query as too complex.
        if automaton.is_too_complex() {
            return true;
        }
        let start = StateSet::singleton(automaton.start());
        let accept = automaton.accept();
        match producer {
            // A token has no children, not even extras: it realizes `p` only if `p`
            // accepts the empty child sequence.
            Producer::Leaf => eps_closure(automaton, &start).contains(accept),
            // Some production of the variable threads `A_p` from start to accept. These
            // are the node's own children, so no field is inherited from above.
            Producer::Var(v) => {
                let production_count = frozen.variable(v).productions.len();
                (0..production_count).any(|i| {
                    let production = &frozen.variable(v).productions[i];
                    self.thread_production(frozen, p, production, &start, None)
                        .contains(accept)
                })
            }
        }
    }

    fn compute_thread(&mut self, frozen: &Frozen, (p, h, q, inherited): ThreadKey) -> StateSet {
        let start = StateSet::singleton(q);
        let mut reached = StateSet::default();
        let production_count = frozen.variable(h).productions.len();
        for i in 0..production_count {
            let production = &frozen.variable(h).productions[i];
            let states = self.thread_production(frozen, p, production, &start, inherited);
            reached.union_with(&states);
        }
        reached
    }

    /// Thread one production's steps, left to right, through `A_p` from `start`,
    /// returning the reachable state set. `inherited` is the field a hidden ancestor
    /// step pushed onto this frontier — `None` for a node's own children.
    fn thread_production(
        &mut self,
        frozen: &Frozen,
        p: PatternId,
        production: &[SkeletonStep],
        start: &StateSet,
        inherited: Option<NodeFieldId>,
    ) -> StateSet {
        let automaton = frozen.automaton(p);
        // Effective skip gap per state, recomputed by each `closure` and read by the next
        // `thread_step`. Sized once to the automaton; states outside the live frontier are
        // never read, so stale entries between rounds do not matter.
        let mut gaps = vec![GapClass::Any; automaton.state_count()];
        let mut current = self.closure(frozen, automaton, start, &mut gaps);
        for step in production {
            // A dead frontier stays dead; the rest of the production cannot revive it.
            if current.is_empty() {
                break;
            }
            current = self.thread_step(frozen, p, &current, &gaps, step, inherited);
            let automaton = frozen.automaton(p);
            current = self.closure(frozen, automaton, &current, &mut gaps);
        }
        current
    }

    fn thread_step(
        &mut self,
        frozen: &Frozen,
        p: PatternId,
        current: &StateSet,
        gaps: &[GapClass],
        step: &SkeletonStep,
        inherited: Option<NodeFieldId>,
    ) -> StateSet {
        let automaton = frozen.automaton(p);
        match frozen.classify(step) {
            // A real child of kind `kind`. Each current state either skips it through a
            // gap self-loop, or consumes it through an edge whose kind and field both
            // admit the step. The step's own label wins over an inherited one.
            StepClass::Visible {
                kind,
                field,
                producer,
            } => {
                let effective = field.or(inherited);
                // The query asserts this field absent (`-field`); a production binding it
                // gives the node a forbidden child, so this whole path is dead — it can be
                // neither consumed nor skipped past.
                if automaton.negates(effective) {
                    return StateSet::default();
                }
                let anonymous = frozen.ctx.grammar.is_anonymous_node(kind);
                let extra = frozen.ctx.grammar.is_extra(kind);
                let mut next = StateSet::default();
                for q in current.iter() {
                    self.charge();
                    // The state's *effective* gap (tightest erasure path that reaches it),
                    // so a strict anchor erased into this position still forbids the skip.
                    if gaps[q as usize].admits(anonymous, extra) {
                        next.insert(q);
                    }
                    for (matcher, to) in automaton.pattern_edges(q) {
                        if frozen.kind_ok(matcher.kind, kind)
                            && field_ok(matcher.field, effective)
                            && self.child_sat(frozen, matcher, producer)
                        {
                            next.insert(*to);
                        }
                    }
                }
                next
            }
            // Splice the hidden variable's visible frontier in, pushing down the label it
            // inherits: this step's own field if it has one, otherwise the one already
            // inherited (a plain supertype link never relabels what it carries).
            StepClass::HiddenSubtree(h) => {
                let pushed = step.field.or(inherited);
                let mut next = StateSet::default();
                for q in current.iter() {
                    let reached = self.get_thread((p, h, q, pushed));
                    next.union_with(&reached);
                }
                next
            }
            // A hidden token surfaces nothing and consumes nothing.
            StepClass::HiddenLeaf => current.clone(),
        }
    }

    /// Whether the matched child's own structure is realized by `producer` — trivially
    /// true when the child is childless.
    fn child_sat(&mut self, _frozen: &Frozen, matcher: &ChildMatcher, producer: Producer) -> bool {
        match matcher.child {
            None => true,
            Some(child) => self.get_sat((child, producer)),
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
        // (a skip is open if any path opens it). So a strict anchor survives erasure — a
        // state reached only by erasing optionals under `.!` cannot then skip what the
        // anchor forbids — while a state also reachable by consuming keeps its own gap.
        let mut result = set.clone();
        let mut stack: Vec<State> = Vec::new();
        for q in result.iter() {
            gaps[q as usize] = automaton.gap(q);
            stack.push(q);
        }
        while let Some(q) = stack.pop() {
            self.charge();
            let gq = gaps[q as usize];
            for &to in automaton.eps_edges(q) {
                let candidate = gq.tighten(automaton.gap(to));
                if relax_into(gaps, &mut result, to, candidate) {
                    stack.push(to);
                }
            }
            for (matcher, to) in automaton.pattern_edges(q) {
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

    fn can_consume_extra(&mut self, frozen: &Frozen, matcher: &ChildMatcher) -> bool {
        // Extras are inserted unfielded, so a `field:` matcher can never be one — letting
        // it "consume" an extra here would skip its field constraint.
        if matcher.field.is_some() {
            return false;
        }
        let Some(child) = matcher.child else {
            // Childless matcher: it consumes an extra iff it admits any extra kind. No
            // subtree to realize, so answer in O(1) without building the admitted list —
            // the hot path on wide wildcard child lists.
            return frozen.admits_any_extra(matcher.kind);
        };
        for extra in frozen.extras_admitted_by(matcher.kind) {
            let realized = frozen
                .producers_of(extra)
                .to_vec()
                .into_iter()
                .any(|producer| self.get_sat((child, producer)));
            if realized {
                return true;
            }
        }
        false
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

/// Index every kind to the producers that can build it: the variable named for the
/// kind, and every step occurrence that surfaces it (aliases included).
fn build_producers(grammar: &Grammar) -> HashMap<NodeKindId, Vec<Producer>> {
    let mut producers: HashMap<NodeKindId, Vec<Producer>> = HashMap::new();
    let push = |map: &mut HashMap<NodeKindId, Vec<Producer>>, kind, producer| {
        let entry = map.entry(kind).or_default();
        if !entry.contains(&producer) {
            entry.push(producer);
        }
    };
    for (var_id, variable) in grammar.structure().iter() {
        if let Some(kind) = variable.id {
            push(&mut producers, kind, Producer::Var(var_id));
        }
        for production in &variable.productions {
            for step in production {
                if let Some(kind) = step.target.id {
                    push(&mut producers, kind, Producer::of_step(step));
                }
            }
        }
    }
    producers
}

fn extra_kinds(grammar: &Grammar) -> (Vec<NodeKindId>, Vec<NodeKindId>) {
    // Extras are mostly lexical tokens (comments), so they live in the grammar's
    // extra set, not the syntax-variable structure.
    let extras = grammar.extra_node_kinds().to_vec();
    let named = extras
        .iter()
        .copied()
        .filter(|&kind| !grammar.is_anonymous_node(kind))
        .collect();
    (extras, named)
}
