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
//! comes from the finite domains and monotonicity (Knaster–Tarski) — there is no
//! step budget and no give-up path, so every query gets a definite verdict.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::compiler::analyze::Located;
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
}

pub(super) struct Satisfier<'a> {
    frozen: Frozen<'a>,
    solve: Solve,
}

impl<'a> Satisfier<'a> {
    pub(super) fn new(ctx: AutomatonContext<'a>, relax: bool) -> Self {
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
            },
            solve: Solve::default(),
        }
    }

    /// Whether the grammar can build a node of `kind` whose children realize `node`.
    /// Errs toward `true` (accept) whenever the question cannot be decided, so a
    /// rejection is always sound.
    pub(super) fn satisfiable(&mut self, node: &Located<NodePattern>, kind: NodeKindId) -> bool {
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
        producers
            .iter()
            .any(|&producer| self.solve.sat_value((p, producer)))
    }

    /// Build every automaton interned so far (transitively reaching all child
    /// patterns), so the solve phase reads a frozen automaton set.
    fn build_pending(&mut self) {
        while self.frozen.automata.len() < self.frozen.table.len() {
            let index = self.frozen.automata.len();
            let node = self.frozen.table.node_at(index).clone();
            let automaton =
                automaton::build(&node, self.frozen.ctx, &mut self.frozen.table, self.frozen.relax);
            self.frozen.automata.push(automaton);
        }
    }

    fn run(&mut self) {
        while let Some(key) = self.solve.dequeue() {
            if self.solve.recompute(&self.frozen, key) {
                self.solve.requeue_dependents(key);
            }
        }
    }

    pub(super) fn context(&self) -> AutomatonContext<'a> {
        self.frozen.ctx
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
        if automaton.is_indeterminate() {
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
        let mut current = self.closure(frozen, automaton, start);
        for step in production {
            // A dead frontier stays dead; the rest of the production cannot revive it.
            if current.is_empty() {
                break;
            }
            current = self.thread_step(frozen, p, &current, step, inherited);
            let automaton = frozen.automaton(p);
            current = self.closure(frozen, automaton, &current);
        }
        current
    }

    fn thread_step(
        &mut self,
        frozen: &Frozen,
        p: PatternId,
        current: &StateSet,
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
                let anonymous = frozen.ctx.grammar.is_anonymous_node(kind);
                let extra = frozen.ctx.grammar.is_extra(kind);
                let mut next = StateSet::default();
                for q in current.iter() {
                    if automaton.gap(q).admits(anonymous, extra) {
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
    fn closure(&mut self, frozen: &Frozen, automaton: &ChildAutomaton, set: &StateSet) -> StateSet {
        let mut result = eps_closure(automaton, set);
        loop {
            let mut grew = false;
            for q in result.iter().collect::<Vec<_>>() {
                for (matcher, to) in automaton.pattern_edges(q) {
                    if result.contains(*to) {
                        continue;
                    }
                    if self.can_consume_extra(frozen, matcher) && result.insert(*to) {
                        grew = true;
                    }
                }
            }
            if !grew {
                return result;
            }
            result = eps_closure(automaton, &result);
        }
    }

    fn can_consume_extra(&mut self, frozen: &Frozen, matcher: &ChildMatcher) -> bool {
        // Extras are inserted unfielded, so a `field:` matcher can never be one — letting
        // it "consume" an extra here would skip its field constraint.
        if matcher.field.is_some() {
            return false;
        }
        for extra in frozen.extras_admitted_by(matcher.kind) {
            let realized = match matcher.child {
                None => true,
                Some(child) => frozen
                    .producers_of(extra)
                    .to_vec()
                    .into_iter()
                    .any(|producer| self.get_sat((child, producer))),
            };
            if realized {
                return true;
            }
        }
        false
    }
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
