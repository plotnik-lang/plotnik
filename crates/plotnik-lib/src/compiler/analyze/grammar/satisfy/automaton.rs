//! The per-query-node child automaton `A_p`.
//!
//! For a query node pattern `p`, `A_p` is a small NFA over `p`'s child list. The
//! grammar's productions are *threaded* through it (see `engine.rs`): every visible
//! child a production emits must be consumed, either by a **pattern edge** (a query
//! child it matches) or by a **gap edge** (a self-loop the anchor context lets it
//! skip). Reaching an accept state means the production's children realize what `p`
//! demands. Quantifiers become loops, alternations become parallel branches, and a
//! nested sequence inlines its items — no collapsing, so arity is exact.
//!
//! The construction reuses `compute_nav_modes` / `check_trailing_anchor` verbatim,
//! so the gap classes the checker reasons over are the same navs codegen emits.

use std::collections::HashMap;

use rowan::TextRange;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::{GapClass, check_trailing_anchor, compute_nav_modes};
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::{
    self, NodePattern, Pattern, QuantifierKind, SeqItem, TokenPattern, token_src,
};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKindId};

/// A query node pattern interned to a dense id, keyed by source location so a
/// definition reached from many sites — or a recursive one — collapses to one id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PatternId(u32);

impl PatternId {
    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
}

/// An automaton state index, dense within one `A_p`.
pub(super) type State = u32;

/// What one query child position demands of a single grammar child.
#[derive(Clone, Copy, Debug)]
pub(super) enum KindConstraint {
    /// `(kind)` or a reference resolving to one — a concrete named or anonymous kind.
    Exact(NodeKindId),
    /// `(_)` — any named node.
    AnyNamed,
    /// `_` — any node, named or anonymous.
    AnyNode,
    /// A kind the checker could not pin (unresolved literal, `ERROR`/`MISSING`).
    /// It never narrows: matching anything keeps rejection sound.
    Unconstrained,
}

/// A pattern-edge label: the kind it accepts and the child whose own structure the
/// matched grammar producer must in turn realize.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChildMatcher {
    pub(super) kind: KindConstraint,
    /// The child node whose child structure the grammar producer must realize, or
    /// `None` when the child is childless (a bare node, token, or wildcard) and
    /// matching its kind is the whole constraint.
    pub(super) child: Option<PatternId>,
    /// The field the matched child must bind, when the query wrote `field: …`. The
    /// engine enforces it only against a node's *direct* children, where the grammar's
    /// step field is authoritative; a field surfacing through a hidden rule is left
    /// unconstrained (its inner-vs-outer label is ambiguous), so a rejection is sound.
    pub(super) field: Option<NodeFieldId>,
}

#[derive(Debug)]
struct StateData {
    /// The gap self-loop: nodes a production may skip here without leaving the state.
    gap: GapClass,
    pattern_edges: Vec<(ChildMatcher, State)>,
    eps_edges: Vec<State>,
}

impl StateData {
    fn new(gap: GapClass) -> Self {
        Self {
            gap,
            pattern_edges: Vec::new(),
            eps_edges: Vec::new(),
        }
    }
}

/// The child automaton of one query node pattern.
#[derive(Debug)]
pub(super) struct ChildAutomaton {
    states: Vec<StateData>,
    start: State,
    accept: State,
    /// Fields the query asserts absent (`-field`). A production step that binds one
    /// would give the node a child the query forbids, so threading kills that path.
    negated_fields: Vec<NodeFieldId>,
    /// Set when the query side could not be represented finitely (a sibling-recursive
    /// definition splicing siblings). The engine treats it as satisfiable — sound,
    /// since we then never reject.
    indeterminate: bool,
}

impl ChildAutomaton {
    pub(super) fn start(&self) -> State {
        self.start
    }

    pub(super) fn accept(&self) -> State {
        self.accept
    }

    /// Whether the query forbids `field` on this node via `-field`.
    pub(super) fn negates(&self, field: Option<NodeFieldId>) -> bool {
        field.is_some_and(|f| self.negated_fields.contains(&f))
    }

    pub(super) fn is_indeterminate(&self) -> bool {
        self.indeterminate
    }

    pub(super) fn gap(&self, state: State) -> GapClass {
        self.states[state as usize].gap
    }

    pub(super) fn pattern_edges(&self, state: State) -> &[(ChildMatcher, State)] {
        &self.states[state as usize].pattern_edges
    }

    pub(super) fn eps_edges(&self, state: State) -> &[State] {
        &self.states[state as usize].eps_edges
    }
}

/// The read-only grammar/query context an automaton build needs.
#[derive(Clone, Copy)]
pub(super) struct AutomatonContext<'a> {
    pub(super) grammar: &'a Grammar,
    pub(super) symbol_table: &'a SymbolTable,
    pub(super) source_map: &'a SourceMap,
}

impl<'a> AutomatonContext<'a> {
    pub(super) fn content(&self, source: SourceId) -> &'a str {
        self.source_map.content(source)
    }
}

/// Interns query node patterns to [`PatternId`]s. Keyed by `(source, range)` so the
/// same definition body interns once however many references reach it.
#[derive(Default)]
pub(super) struct PatternTable {
    by_loc: HashMap<(SourceId, TextRange), PatternId>,
    nodes: Vec<Located<NodePattern>>,
}

impl PatternTable {
    pub(super) fn intern(&mut self, node: Located<NodePattern>) -> PatternId {
        let key = (node.source(), node.node().text_range());
        if let Some(&id) = self.by_loc.get(&key) {
            return id;
        }
        let id = PatternId(self.nodes.len() as u32);
        self.by_loc.insert(key, id);
        self.nodes.push(node);
        id
    }

    pub(super) fn node_at(&self, index: usize) -> &Located<NodePattern> {
        &self.nodes[index]
    }

    pub(super) fn len(&self) -> usize {
        self.nodes.len()
    }
}

/// Builds `A_p` from a node pattern, interning child node patterns into `table`.
/// `relax` widens every gap to [`GapClass::Any`] — the diagnostic probe uses it to
/// ask whether the anchors alone are the obstacle.
pub(super) fn build(
    node: &Located<NodePattern>,
    ctx: AutomatonContext<'_>,
    table: &mut PatternTable,
    relax: bool,
) -> ChildAutomaton {
    let mut builder = Builder {
        ctx,
        table,
        states: Vec::new(),
        indeterminate: false,
        ref_stack: Vec::new(),
    };
    let start = builder.new_state(GapClass::Any);
    let items: Vec<SeqItem> = node.node().items().collect();
    let accept = builder.emit_items(&items, Descent::root(node.source()), true, start);

    let (has_trailing, trailing_nav) = check_trailing_anchor(&items, ctx.symbol_table);
    let trailing_gap = if has_trailing {
        trailing_nav.and_then(GapClass::from_nav).unwrap_or(GapClass::Any)
    } else {
        GapClass::Any
    };
    builder.states[accept as usize].gap = trailing_gap;

    let mut states = builder.states;
    let indeterminate = builder.indeterminate;
    if relax {
        for state in &mut states {
            state.gap = GapClass::Any;
        }
    }
    ChildAutomaton {
        states,
        start,
        accept,
        negated_fields: negated_fields(node, ctx),
        indeterminate,
    }
}

/// The fields a node pattern asserts absent through `-field` items, resolved to ids.
fn negated_fields(node: &Located<NodePattern>, ctx: AutomatonContext<'_>) -> Vec<NodeFieldId> {
    node.node()
        .syntax()
        .children()
        .filter_map(ast::NegatedField::cast)
        .filter_map(|neg| neg.name())
        .filter_map(|name| ctx.grammar.resolve_field(name.text()))
        .collect()
}

struct Builder<'a, 'b> {
    ctx: AutomatonContext<'a>,
    table: &'b mut PatternTable,
    states: Vec<StateData>,
    indeterminate: bool,
    /// Definition names currently being inlined, to catch sibling-recursive refs
    /// that would splice siblings without bound.
    ref_stack: Vec<String>,
}

/// The context one query position is lowered under. `source` is the file its text
/// lives in — it changes when a reference inlines a definition from elsewhere — and
/// `field` is the label in force from an enclosing `field: …`, which the matcher this
/// position builds must bind. Both ride the descent together, transformed only through
/// the named transitions below so the lowering reads as "lower this position here".
#[derive(Clone, Copy)]
struct Descent {
    source: SourceId,
    field: Option<NodeFieldId>,
}

impl Descent {
    fn root(source: SourceId) -> Self {
        Self {
            source,
            field: None,
        }
    }

    /// Strip the field label: the items of a sibling sequence each take their own, so
    /// a field never carries across a `{…}` onto the positions inside it.
    fn bare(self) -> Self {
        Self {
            field: None,
            ..self
        }
    }

    /// Enter a `field: …`. The innermost label wins, so an absent or unresolved inner
    /// field leaves the inherited one in force.
    fn under(self, field: Option<NodeFieldId>) -> Self {
        match field {
            Some(_) => Self { field, ..self },
            None => self,
        }
    }

    /// Cross into a referenced definition: its text lives in `source`, but the label
    /// the reference site asked for still binds the target node.
    fn into_ref(self, source: SourceId) -> Self {
        Self { source, ..self }
    }
}

impl Builder<'_, '_> {
    fn new_state(&mut self, gap: GapClass) -> State {
        let id = self.states.len() as State;
        self.states.push(StateData::new(gap));
        id
    }

    /// Thread a flat item list (a node's children, or an inlined sequence) onto the
    /// spine starting at `entry`, stamping each pattern's leading gap from the shared
    /// nav computation. Returns the exit state. Each item descends field-fresh — a
    /// sibling's label comes only from its own `field: …`, never the list's context.
    fn emit_items(
        &mut self,
        items: &[SeqItem],
        descent: Descent,
        inside_node: bool,
        entry: State,
    ) -> State {
        let navs = compute_nav_modes(items, inside_node, self.ctx.symbol_table);
        let mut navs = navs.into_iter();
        let mut cur = entry;
        for item in items {
            let SeqItem::Pattern(pattern) = item else {
                continue;
            };
            let (_, nav) = navs
                .next()
                .expect("compute_nav_modes yields one entry per pattern item");
            let gap = nav.and_then(GapClass::from_nav).unwrap_or(GapClass::Any);
            self.states[cur as usize].gap = satisfiability_gap(gap, pattern);
            cur = self.emit_pattern(pattern, descent.bare(), cur);
        }
        cur
    }

    /// Emit one item between `from` and the returned exit state. The gap *before* this
    /// item is already stamped on `from`; `descent` carries the source and the field label
    /// the matcher this item builds must bind.
    fn emit_pattern(&mut self, pattern: &Pattern, descent: Descent, from: State) -> State {
        match pattern {
            Pattern::CapturedPattern(cap) => match cap.inner() {
                Some(inner) => self.emit_pattern(&inner, descent, from),
                None => from,
            },
            Pattern::NodePattern(node) => {
                let matcher = self.node_matcher(node, descent);
                self.emit_single(matcher, from)
            }
            Pattern::TokenPattern(token) => {
                let matcher = self.token_matcher(token, descent);
                self.emit_single(matcher, from)
            }
            Pattern::FieldPattern(field_pattern) => {
                let field = field_pattern
                    .name()
                    .and_then(|name| self.ctx.grammar.resolve_field(name.text()));
                match field_pattern.value() {
                    Some(value) => self.emit_pattern(&value, descent.under(field), from),
                    None => from,
                }
            }
            Pattern::QuantifiedPattern(q) => self.emit_quantifier(q, descent, from),
            Pattern::Union(_) | Pattern::Enum(_) => self.emit_alternation(pattern, descent, from),
            // A sequence is several siblings, never a single field value (the grammar
            // forbids `field: {…}`), so the field does not carry into its items.
            Pattern::SeqPattern(seq) => {
                let items: Vec<SeqItem> = seq.items().collect();
                self.emit_items(&items, descent, false, from)
            }
            Pattern::DefRef(def_ref) => self.emit_def_ref(def_ref, descent, from),
        }
    }

    fn emit_single(&mut self, matcher: ChildMatcher, from: State) -> State {
        let to = self.new_state(GapClass::Any);
        self.states[from as usize].pattern_edges.push((matcher, to));
        to
    }

    fn emit_quantifier(&mut self, q: &ast::QuantifiedPattern, descent: Descent, from: State) -> State {
        let Some(inner) = q.inner() else {
            return from;
        };
        // The repeat iterates at `from`, which already carries the entry gap; the VM's
        // sibling continuation preserves the skip class, so looping back to `from`
        // reuses the same between-repetition gap the VM would.
        let kind = q.quantifier_kind();
        match kind {
            Some(QuantifierKind::Optional) => {
                let to = self.emit_pattern(&inner, descent, from);
                self.states[from as usize].eps_edges.push(to);
                to
            }
            Some(QuantifierKind::ZeroOrMore) => {
                let body_exit = self.emit_pattern(&inner, descent, from);
                self.states[body_exit as usize].eps_edges.push(from);
                let to = self.new_state(GapClass::Any);
                self.states[from as usize].eps_edges.push(to);
                to
            }
            Some(QuantifierKind::OneOrMore) => {
                let body_exit = self.emit_pattern(&inner, descent, from);
                self.states[body_exit as usize].eps_edges.push(from);
                let to = self.new_state(GapClass::Any);
                self.states[body_exit as usize].eps_edges.push(to);
                to
            }
            // A malformed quantifier with no operator imposes nothing.
            None => self.emit_pattern(&inner, descent, from),
        }
    }

    fn emit_alternation(&mut self, alt: &Pattern, descent: Descent, from: State) -> State {
        let to = self.new_state(GapClass::Any);
        let mut had_branch = false;
        for branch in alt.children() {
            had_branch = true;
            let branch_exit = self.emit_pattern(&branch, descent, from);
            self.states[branch_exit as usize].eps_edges.push(to);
        }
        if !had_branch {
            self.states[from as usize].eps_edges.push(to);
        }
        to
    }

    fn emit_def_ref(&mut self, def_ref: &ast::DefRef, descent: Descent, from: State) -> State {
        let Some(name_token) = def_ref.name() else {
            return self.emit_single(unconstrained_matcher(descent.field), from);
        };
        let name = name_token.text();
        // A reference that splices siblings into the parent (`Seq = {(a) (Seq)}`) makes
        // the child language non-regular; abandon finite construction and accept.
        if self.ref_stack.iter().any(|n| n == name) {
            self.indeterminate = true;
            return from;
        }
        let Some(target) = self.ctx.symbol_table.located_definition(name) else {
            return self.emit_single(unconstrained_matcher(descent.field), from);
        };
        let descent = descent.into_ref(target.source());

        // A reference to a single node is an atomic child: one matcher whose body is
        // the referenced node, so its own structure is checked against the producer.
        if let Pattern::NodePattern(node) = target.node() {
            let matcher = self.node_matcher(node, descent);
            return self.emit_single(matcher, from);
        }

        // Otherwise the reference's body inlines its structure (an alternation of
        // kinds, a grouped sequence) directly into this position.
        self.ref_stack.push(name.to_string());
        let exit = self.emit_pattern(target.node(), descent, from);
        self.ref_stack.pop();
        exit
    }

    fn node_matcher(&mut self, node: &NodePattern, descent: Descent) -> ChildMatcher {
        let kind = self.node_kind(node, descent.source);
        let child = node
            .items()
            .next()
            .is_some()
            .then(|| self.table.intern(Located::new(descent.source, node.clone())));
        ChildMatcher {
            kind,
            child,
            field: descent.field,
        }
    }

    fn token_matcher(&self, token: &TokenPattern, descent: Descent) -> ChildMatcher {
        ChildMatcher {
            kind: self.token_kind(token, descent.source),
            child: None,
            field: descent.field,
        }
    }

    fn node_kind(&self, node: &NodePattern, source: SourceId) -> KindConstraint {
        if node.is_any() {
            return KindConstraint::AnyNamed;
        }
        let Some(type_token) = node.kind_token() else {
            return KindConstraint::Unconstrained;
        };
        // ERROR/MISSING are always considered matchable; structural validation does
        // not reason about error trees.
        if matches!(type_token.kind(), SyntaxKind::KwError | SyntaxKind::KwMissing) {
            return KindConstraint::Unconstrained;
        }
        let text = token_src(&type_token, self.ctx.content(source));
        match self.ctx.grammar.resolve_named_node(text) {
            Some(id) => KindConstraint::Exact(id),
            // The resolution pass already reported the unknown kind; accept here.
            None => KindConstraint::Unconstrained,
        }
    }

    fn token_kind(&self, token: &TokenPattern, source: SourceId) -> KindConstraint {
        if token.is_any() {
            return KindConstraint::AnyNode;
        }
        let Some(value_token) = token.value() else {
            return KindConstraint::Unconstrained;
        };
        let text = token_src(&value_token, self.ctx.content(source));
        match self.ctx.grammar.resolve_anonymous_node(text) {
            Some(id) => KindConstraint::Exact(id),
            None => KindConstraint::Unconstrained,
        }
    }
}

fn unconstrained_matcher(field: Option<NodeFieldId>) -> ChildMatcher {
    ChildMatcher {
        kind: KindConstraint::Unconstrained,
        child: None,
        field,
    }
}

/// Widen a narrow skip to the broad one for any non-anonymous-token position. After
/// an alternation the VM computes per-branch navs, so a named branch may skip an
/// anonymous token the conservative whole-pattern nav would not; the checker reasons
/// over all branches at once and so takes the most permissive gap. Strict (`Nothing`)
/// is never widened — it is the user's adjacency demand.
fn satisfiability_gap(gap: GapClass, pattern: &Pattern) -> GapClass {
    if gap == GapClass::ExtrasOnly && !is_plain_anonymous(pattern) {
        GapClass::AnonAndExtras
    } else {
        gap
    }
}

fn is_plain_anonymous(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::TokenPattern(token) => !token.is_any(),
        Pattern::CapturedPattern(cap) => cap.inner().as_ref().is_some_and(is_plain_anonymous),
        _ => false,
    }
}
