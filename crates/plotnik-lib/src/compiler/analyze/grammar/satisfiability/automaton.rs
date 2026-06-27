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
//! The construction reuses `AnchorSemantics` verbatim, so the gap classes the checker
//! reasons over are the same navs codegen emits.

use std::collections::HashMap;

use rowan::TextRange;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::{
    AnchorSemantics, GapClass, has_direct_alternation_branch_nav,
};
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::{
    self, NodePattern, Pattern, QuantifierKind, SeqItem, TokenPattern, token_src,
};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKindId};

use super::node_constrains_children;

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
    /// A syntax form the checker intentionally does not pin (`ERROR`/`MISSING`, or
    /// parser recovery without a token). It never narrows: matching anything keeps
    /// rejection sound.
    Unconstrained,
}

/// A pattern-edge label: the kind it accepts and the child whose own structure the
/// matched grammar realizer must in turn realize.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChildMatcher {
    pub(super) kind: KindConstraint,
    /// The nested query pattern whose child structure the grammar realizer must
    /// satisfy, or `None` when matching this kind is the whole constraint.
    pub(super) nested_pattern: Option<PatternId>,
    /// The field the matched child must bind, when the query wrote `field: …`. The
    /// engine compares it with the grammar step's effective field: the step's own
    /// field, or the field inherited while surfacing a visible child through hidden
    /// structure.
    pub(super) field: Option<NodeFieldId>,
}

impl ChildMatcher {
    fn any_sibling() -> Self {
        Self {
            kind: KindConstraint::AnyNode,
            nested_pattern: None,
            field: None,
        }
    }

    fn node(
        kind: KindConstraint,
        nested_pattern: Option<PatternId>,
        field: Option<NodeFieldId>,
    ) -> Self {
        Self {
            kind,
            nested_pattern,
            field,
        }
    }

    fn token(kind: KindConstraint, field: Option<NodeFieldId>) -> Self {
        Self {
            kind,
            nested_pattern: None,
            field,
        }
    }

    fn unconstrained(field: Option<NodeFieldId>) -> Self {
        Self {
            kind: KindConstraint::Unconstrained,
            nested_pattern: None,
            field,
        }
    }
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
    /// Set when construction hit a resource ceiling — too many states (an exponentially
    /// widening expansion) or too-deep recursion. This is the query asking for more than
    /// we will spend, so it is *rejected* with a clear "too complex" diagnostic rather
    /// than judged on an automaton we declined to finish.
    too_complex: bool,
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

    /// Whether construction bailed on a resource ceiling (state cap or depth), meaning
    /// the query must be rejected as too complex to compile.
    pub(super) fn is_too_complex(&self) -> bool {
        self.too_complex
    }

    pub(super) fn gap(&self, state: State) -> GapClass {
        self.states[state as usize].gap
    }

    pub(super) fn state_count(&self) -> usize {
        self.states.len()
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

/// Whether construction should preserve query anchors or widen every gap for the
/// diagnostic probe that asks "would this match without the anchors?"
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AnchorMode {
    Enforce,
    Relax,
}

impl AnchorMode {
    fn relaxes(self) -> bool {
        matches!(self, Self::Relax)
    }
}

/// The sibling context for anchor navigation. Boundary anchors only become `Down`
/// navs inside a node's own child list; a spliced `{...}` sequence inherits its
/// outer gap instead.
#[derive(Clone, Copy)]
enum NavContext {
    NodeChildren,
    SplicedSequence,
}

impl NavContext {
    fn is_inside_node(self) -> bool {
        matches!(self, Self::NodeChildren)
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
/// [`AnchorMode::Relax`] widens every gap to [`GapClass::Any`] so the diagnostic
/// probe can ask whether anchors alone are the obstacle.
pub(super) fn build(
    node: &Located<NodePattern>,
    ctx: AutomatonContext<'_>,
    table: &mut PatternTable,
    anchor_mode: AnchorMode,
    max_depth: u32,
) -> ChildAutomaton {
    let anchor_semantics = AnchorSemantics::new(ctx.symbol_table);
    let mut builder = Builder {
        ctx,
        table,
        anchor_semantics: &anchor_semantics,
        states: Vec::new(),
        too_complex: false,
        ref_stack: Vec::new(),
        max_depth,
        depth: 0,
    };
    let start = builder.new_state(GapClass::Any);
    let items = ItemList::node_children(node.node());
    let accept = builder.emit_items(&items, Descent::root(node.source()), start);

    let (has_trailing, trailing_nav) = builder
        .anchor_semantics
        .check_trailing_anchor(items.as_slice());
    let trailing_gap = if has_trailing {
        trailing_nav
            .and_then(GapClass::from_nav)
            .unwrap_or(GapClass::Any)
    } else {
        GapClass::Any
    };
    builder.states[accept as usize].gap = trailing_gap;

    let mut states = builder.states;
    let too_complex = builder.too_complex;
    if anchor_mode.relaxes() {
        for state in &mut states {
            state.gap = GapClass::Any;
        }
    }
    ChildAutomaton {
        states,
        start,
        accept,
        negated_fields: negated_fields(node, ctx),
        too_complex,
    }
}

/// The fields a node pattern asserts absent through `-field` items, resolved to ids.
fn negated_fields(node: &Located<NodePattern>, ctx: AutomatonContext<'_>) -> Vec<NodeFieldId> {
    node.node()
        .syntax()
        .children()
        .filter_map(ast::NegatedField::cast)
        .map(|neg| {
            let name = neg
                .name()
                .expect("admitted negated field must have a field name");
            checked_field(ctx, name.text())
        })
        .collect()
}

struct Builder<'a, 'b> {
    ctx: AutomatonContext<'a>,
    table: &'b mut PatternTable,
    anchor_semantics: &'b AnchorSemantics<'a>,
    states: Vec<StateData>,
    /// Set when a resource ceiling (state cap or recursion depth) is hit — the query is
    /// rejected as too complex, not accepted. Construction short-circuits once it is set.
    too_complex: bool,
    /// Definition names currently being inlined, to catch sibling-recursive refs
    /// that would splice siblings without bound.
    ref_stack: Vec<String>,
    /// The structural-depth ceiling — the parser's own `max_depth`. Inlining a long
    /// chain of references (`A = {(B)}`, `B = {(C)}`, …) expands the tree past any
    /// nesting the parser admitted, and the recursion that builds it is native; left
    /// unbounded it overflows the stack. Capping construction at the depth the parser
    /// already survived keeps the two in lockstep — if it parsed, it builds.
    max_depth: u32,
    /// Current [`Self::emit_pattern`] recursion depth, checked against `max_depth`.
    depth: u32,
}

struct ItemList {
    items: Vec<SeqItem>,
    nav_context: NavContext,
    first_gap: Option<GapClass>,
}

impl ItemList {
    fn node_children(node: &NodePattern) -> Self {
        Self {
            items: node.items().collect(),
            nav_context: NavContext::NodeChildren,
            first_gap: None,
        }
    }

    fn spliced_sequence(seq: &ast::SeqPattern, first_gap: GapClass) -> Self {
        Self {
            items: seq.items().collect(),
            nav_context: NavContext::SplicedSequence,
            first_gap: Some(first_gap),
        }
    }

    fn as_slice(&self) -> &[SeqItem] {
        &self.items
    }
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

    /// Enter a `field: …`. The innermost label wins.
    fn under_field(self, field: NodeFieldId) -> Self {
        Self {
            field: Some(field),
            ..self
        }
    }

    /// Cross into a referenced definition: its text lives in `source`, but the label
    /// the reference site asked for still binds the target node.
    fn into_ref(self, source: SourceId) -> Self {
        Self { source, ..self }
    }
}

/// Upper bound on one automaton's states. A definition that splices siblings into
/// itself through ever-widening expansions (`A = [(B)(B)]`, `B = [(C)(C)]`, …) demands
/// exponentially many states; rather than chase that to an OOM, construction stops and
/// flags the query too complex to compile — a clean rejection, not a silent accept.
/// Far above any real pattern's width, which is a few hundred children at most. This is
/// the heap counterpart to the depth cap: depth guards the stack, the state cap guards
/// the heap, and both reject rather than spend unboundedly.
const STATE_CAP: usize = 20_000;

impl Builder<'_, '_> {
    fn new_state(&mut self, gap: GapClass) -> State {
        if self.states.len() >= STATE_CAP {
            // Stop growing the automaton: record that we bailed on a resource ceiling so the
            // query is rejected as too complex rather than judged on a half-built automaton.
            self.too_complex = true;
        }
        let id = self.states.len() as State;
        self.states.push(StateData::new(gap));
        id
    }

    /// Thread a flat item list (a node's children, or an inlined sequence) onto the
    /// spine starting at `entry`, stamping each pattern's leading gap from the shared
    /// nav computation. Returns the exit state. Each item descends field-fresh — a
    /// sibling's label comes only from its own `field: …`, never the list's context.
    fn emit_items(&mut self, items: &ItemList, descent: Descent, entry: State) -> State {
        let navs = self
            .anchor_semantics
            .compute_nav_modes(items.as_slice(), items.nav_context.is_inside_node());
        let mut navs = navs.into_iter();
        let mut cur = entry;
        let mut first = true;
        for item in items.as_slice() {
            let SeqItem::Pattern(pattern) = item else {
                continue;
            };
            let (_, nav) = navs
                .next()
                .expect("compute_nav_modes yields one entry per pattern item");
            // A sequence spliced under an outer anchor (a `{…}` group, a referenced
            // body) inherits that anchor's gap on its first child: the outer level
            // already chose it, so recomputing here would drop the adjacency and let a
            // strict anchor leak through the boundary.
            let gap = match (first, items.first_gap) {
                (true, Some(g)) => g,
                _ => satisfiability_gap(
                    nav.and_then(GapClass::from_nav).unwrap_or(GapClass::Any),
                    pattern,
                ),
            };
            self.states[cur as usize].gap = gap;
            cur = self.emit_pattern(pattern, descent.bare(), cur);
            first = false;
        }
        cur
    }

    /// Emit one item between `from` and the returned exit state, counting recursion
    /// depth. Every descent funnels through here, so one check bounds the whole walk:
    /// outrunning `max_depth` means the inlined expansion is deeper than any tree the
    /// parser would admit, so we stop and flag the query too complex (rejected). An
    /// already-bailing build — from depth or the state cap — short-circuits without doing
    /// more work.
    fn emit_pattern(&mut self, pattern: &Pattern, descent: Descent, from: State) -> State {
        if self.too_complex {
            return from;
        }
        if self.depth >= self.max_depth {
            self.too_complex = true;
            return from;
        }
        self.depth += 1;
        let exit = self.emit_pattern_inner(pattern, descent, from);
        self.depth -= 1;
        exit
    }

    /// Emit one item between `from` and the returned exit state. The gap *before* this
    /// item is already stamped on `from`; `descent` carries the source and the field label
    /// the matcher this item builds must bind.
    fn emit_pattern_inner(&mut self, pattern: &Pattern, descent: Descent, from: State) -> State {
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
            Pattern::FieldPattern(field_pattern) => match field_pattern.value() {
                Some(value) => {
                    let field = self.field_id(field_pattern);
                    self.emit_pattern(&value, descent.under_field(field), from)
                }
                None => from,
            },
            Pattern::QuantifiedPattern(q) => self.emit_quantifier(q, descent, from),
            Pattern::Union(_) | Pattern::Enum(_) => self.emit_alternation(pattern, descent, from),
            // A sequence is several siblings, never a single field value (the grammar
            // forbids `field: {…}`), so the field does not carry into its items.
            Pattern::SeqPattern(seq) => {
                // The group sits at one child position whose gap `from` already carries;
                // thread it as the first child's gap so an enclosing anchor survives the
                // `{…}` boundary instead of being recomputed away.
                let items = ItemList::spliced_sequence(seq, self.states[from as usize].gap);
                self.emit_items(&items, descent, from)
            }
            Pattern::DefRef(def_ref) => self.emit_def_ref(def_ref, descent, from),
        }
    }

    fn emit_single(&mut self, matcher: ChildMatcher, from: State) -> State {
        let to = self.new_state(GapClass::Any);
        self.states[from as usize].pattern_edges.push((matcher, to));
        to
    }

    fn emit_quantifier(
        &mut self,
        q: &ast::QuantifiedPattern,
        descent: Descent,
        from: State,
    ) -> State {
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
            return self.emit_single(ChildMatcher::unconstrained(descent.field), from);
        };
        let name = name_token.text();
        // A reference that splices siblings into the parent (`Seq = {(a) (Seq)}`) makes the
        // child language non-regular, so it cannot inline finitely. Rather than abandon the
        // whole automaton — which would erase the constraints the non-recursive prefix
        // already imposed, e.g. a leading anchor — over-approximate just the recursive tail:
        // a self-loop consuming any remaining sibling. The accepted language only grows
        // (rejection stays sound), while the first-child / anchor constraints still bind.
        if self.ref_stack.iter().any(|n| n == name) {
            self.states[from as usize]
                .pattern_edges
                .push((ChildMatcher::any_sibling(), from));
            return from;
        }
        let target = self
            .ctx
            .symbol_table
            .located_definition(name)
            .expect("admitted definition reference must resolve");
        let descent = descent.into_ref(target.source());

        // A reference to a single node is an atomic child: one matcher whose body is
        // the referenced node, so its own structure is checked against the realizer.
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
        // Intern (build a child automaton for) the node whenever it constrains its
        // children at all. A `-field` has no `items()`, so testing items alone would
        // treat `(pair -key)` as a bare `(pair)` and silently drop the negation.
        let nested_pattern = node_constrains_children(node).then(|| {
            self.table
                .intern(Located::new(descent.source, node.clone()))
        });
        ChildMatcher::node(kind, nested_pattern, descent.field)
    }

    fn token_matcher(&self, token: &TokenPattern, descent: Descent) -> ChildMatcher {
        ChildMatcher::token(self.token_kind(token, descent.source), descent.field)
    }

    fn field_id(&self, field_pattern: &ast::FieldPattern) -> NodeFieldId {
        let name = field_pattern
            .name()
            .expect("admitted field pattern must have a field name");
        checked_field(self.ctx, name.text())
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
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return KindConstraint::Unconstrained;
        }
        let text = token_src(&type_token, self.ctx.content(source));
        KindConstraint::Exact(checked_named_node(self.ctx, text))
    }

    fn token_kind(&self, token: &TokenPattern, source: SourceId) -> KindConstraint {
        if token.is_any() {
            return KindConstraint::AnyNode;
        }
        let Some(value_token) = token.value() else {
            return KindConstraint::Unconstrained;
        };
        let text = token_src(&value_token, self.ctx.content(source));
        KindConstraint::Exact(checked_anonymous_node(self.ctx, text))
    }
}

fn checked_field(ctx: AutomatonContext<'_>, name: &str) -> NodeFieldId {
    ctx.grammar
        .resolve_field(name)
        .expect("admitted field name must resolve")
}

fn checked_named_node(ctx: AutomatonContext<'_>, name: &str) -> NodeKindId {
    ctx.grammar
        .resolve_named_node(name)
        .expect("admitted named node kind must resolve")
}

fn checked_anonymous_node(ctx: AutomatonContext<'_>, text: &str) -> NodeKindId {
    ctx.grammar
        .resolve_anonymous_node(text)
        .expect("admitted anonymous token kind must resolve")
}

/// Widen a narrow skip to the broad one for direct alternation positions. The VM
/// computes per-branch navs there, so a named branch may skip an anonymous token
/// the conservative whole-pattern nav would not; the checker reasons over all
/// branches at once and so takes the most permissive gap. Strict (`Nothing`) is
/// never widened — it is the user's adjacency demand.
fn satisfiability_gap(gap: GapClass, pattern: &Pattern) -> GapClass {
    if gap == GapClass::ExtrasOnly && has_direct_alternation_branch_nav(pattern) {
        GapClass::AnonymousAndExtras
    } else {
        gap
    }
}
