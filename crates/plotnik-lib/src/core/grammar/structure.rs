//! Distilled, retained structural skeleton of the flattened grammar.
//!
//! `node_shapes` summarizes *what can be inside what* and, in doing so, discards
//! order and adjacency. This module retains what that summary throws away: the
//! ordered production step sequences. Each step is resolved into two independent
//! facts — *what it matches* (a public id) and *what it descends into* (a
//! variable) — built from the same [`GrammarContext`] and reusing `node_shapes`'
//! symbol resolution and `lower`'s public-name normalization so the views cannot
//! drift. It is the substrate for sequence/anchor impossibility (#444) and
//! first-set analysis.
//!
//! # Assumptions
//!
//! These are load-bearing but not sacred — verify them before relying on or
//! extending the table:
//!
//! - **A [`VarId`] is the syntax-variable index.** A [`StepTarget`]'s `body` stores
//!   a non-terminal `Symbol`'s `index`, and [`StructureTable::variables`] is built
//!   in that same order, so the two line up. If tree-sitter's variable order ever
//!   stops matching symbol indices, a `body` would silently point at the wrong
//!   variable. The conformance test guards that every `body` resolves, but does not
//!   prove the index correspondence itself.
//! - **Identity and descent are independent.** A step records `id` (the public kind
//!   it matches, when it surfaces in the tree) and `body` (the variable it expands
//!   into, if any) as two separate facts. Tree-sitter varies them independently, so
//!   fusing them loses information — that is exactly how aliased non-terminals lost
//!   their descent link before.
//! - **Public names are NUL-truncated.** Resolution maps are keyed on
//!   `public_node_kind`, so every name is normalized through that exact
//!   function before lookup; resolving raw names would drop disambiguated kinds.
//! - **Classification mirrors `node_shapes`.** Alias folding, visibility, and symbol
//!   naming reuse the `node_shapes` helpers verbatim, so a step is classified here
//!   exactly as during shape derivation. Forking that logic would let the structural
//!   and shape views disagree.
//! - **The table points; it does not expand.** Tree-sitter publishes some rules
//!   under a public id and inlines others away; supertypes and aliases are recorded
//!   faithfully (via `id` and/or `body`), never pre-expanded. A consumer expands a
//!   supertype via `Grammar::is_supertype` / `subtypes`, or descends through `body`,
//!   on demand — reusing the grammar's single source of truth so the views cannot
//!   drift.
//! - **A visible step is never wholly unknown.** It always resolves to an `id` or a
//!   `body`: a visible token has an id, a visible non-terminal has a body.
//!   `classify_step` debug-asserts this, and the corpus test exercises it across
//!   every shipped grammar; if it ever trips, the classifier has a new gap.
//! - **Built only via `from_raw`, eagerly and owned.** The flattened grammar is gone
//!   by `from_metadata`, so a metadata-only `Grammar` has an empty table. The table
//!   is built for every grammar even with no consumer yet, and `Grammar` clones
//!   deep-copy it; both are provisional, to revisit when a consumer lands.

use crate::core::{NodeFieldId, NodeKindId};

use super::lower::public_node_kind;
use super::node_shapes::{
    ChildType, GrammarContext, effective_step_alias, symbol_node_metadata,
    variable_type_for_child_type,
};
use super::prepared::{ProductionStep, VariableType};
use super::types::Grammar;

/// Index of a syntax variable, aligned with [`StructureTable::variables`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarId(u32);

impl VarId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a production step matches, resolved against the grammar.
///
/// Two independent facts about the matched node, kept separate because tree-sitter
/// varies them independently:
///
/// - `id` — its public identity, when the node surfaces in the tree under a
///   queryable kind: a concrete kind, a resolvable supertype, or an alias such as
///   TypeScript `interface_body`. `None` when it is spliced in without an id of its
///   own (an inlined supertype like go `_type`, or an ordinary hidden rule).
/// - `body` — the variable whose productions it expands into, for on-demand
///   descent. `None` for leaf tokens; `Some` for every non-terminal, so an aliased
///   node keeps its way in.
///
/// All four combinations occur: both set (a visible non-terminal or alias), only
/// `id` (a token), only `body` (an inlined rule), or neither (a hidden token). A
/// step the grammar classifies as visible always has at least one set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTarget {
    pub id: Option<NodeKindId>,
    pub body: Option<VarId>,
}

/// One production step: the node it matches, and the field it binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkeletonStep {
    pub target: StepTarget,
    /// Field this step binds to, if any.
    pub field: Option<NodeFieldId>,
}

/// A grammar variable reduced to its productions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkeletonVariable {
    /// Public name, kept for diagnostics. Hidden variables have no public id, so
    /// the name is the only stable handle on them.
    pub name: String,
    /// Public id when this variable surfaces in the tree under its own kind — a
    /// named or anonymous rule, a non-inlined supertype, or a rule tree-sitter
    /// publishes via a named alias (e.g. swift `_expression`). `None` for variables
    /// spliced away without an id of their own.
    pub id: Option<NodeKindId>,
    pub kind: VariableType,
    /// One inner `Vec` per production, each an ordered list of steps.
    pub productions: Vec<Vec<SkeletonStep>>,
}

/// Ordered structural skeleton of every syntax variable in the grammar.
///
/// Retained from `Grammar::from_raw`'s flattened `SyntaxGrammar` before it is
/// dropped, then resolved against the grammar's id space. Variables are
/// positionally aligned with the non-terminal symbol index, so a [`StepTarget`]'s
/// `body` indexes [`StructureTable::variables`].
#[derive(Clone, Debug, Default)]
pub struct StructureTable {
    variables: Vec<SkeletonVariable>,
}

impl StructureTable {
    pub(super) fn build(ctx: GrammarContext<'_>, grammar: &Grammar) -> Self {
        let variables = ctx
            .syntax
            .variables
            .iter()
            .map(|variable| {
                let name = public_node_kind(&variable.name);
                SkeletonVariable {
                    id: resolve_visible_id(&name, variable.kind, grammar),
                    name,
                    kind: variable.kind,
                    productions: variable
                        .productions
                        .iter()
                        .map(|production| {
                            production
                                .steps
                                .iter()
                                .map(|step| classify_step(step, ctx, grammar))
                                .collect()
                        })
                        .collect(),
                }
            })
            .collect();
        Self { variables }
    }

    /// Distilled productions, positionally aligned with the grammar's syntax
    /// variables (non-terminal symbol index).
    pub fn variables(&self) -> &[SkeletonVariable] {
        &self.variables
    }

    pub fn variable(&self, id: VarId) -> Option<&SkeletonVariable> {
        self.variables.get(id.index())
    }

    /// Each variable paired with its [`VarId`]. The id is otherwise unconstructible
    /// outside this module, so this is how a consumer keys facts (realizers,
    /// first-sets) by the variable a step descends into.
    pub fn iter(&self) -> impl Iterator<Item = (VarId, &SkeletonVariable)> {
        self.variables
            .iter()
            .enumerate()
            .map(|(index, variable)| (VarId(index as u32), variable))
    }
}

fn resolve_visible_id(
    public_name: &str,
    kind: VariableType,
    grammar: &Grammar,
) -> Option<NodeKindId> {
    match kind {
        VariableType::Named => grammar.resolve_named_node(public_name),
        VariableType::Anonymous => grammar.resolve_anonymous_node(public_name),
        // Hidden-classified rules normally have no public id, but tree-sitter still
        // publishes some as named nodes — non-inlined supertypes, and rules surfaced
        // via a named alias (e.g. swift `_expression`). Probe the named map so those
        // carry their id; ordinary hidden rules miss and stay `None`.
        VariableType::Hidden => grammar.resolve_named_node(public_name),
        VariableType::Auxiliary => None,
    }
}

fn classify_step(
    step: &ProductionStep,
    ctx: GrammarContext<'_>,
    grammar: &Grammar,
) -> SkeletonStep {
    // Fold the step's own alias and the grammar's default alias into one effective
    // child type, exactly as node-shape derivation does, so a default-aliased step
    // resolves to the same public kind here and there.
    let child_type = effective_step_alias(step, ctx.aliases)
        .cloned()
        .map_or(ChildType::Normal(step.symbol), ChildType::Aliased);

    let variable_type = variable_type_for_child_type(&child_type, ctx.syntax, ctx.lexical);

    // Identity: the public id this position surfaces under, when it is visible.
    // Resolution maps are keyed on the public name (truncated at tree-sitter's
    // private disambiguator), so normalize before looking up.
    let id = if variable_type.is_visible() {
        let (name, named) = match &child_type {
            ChildType::Aliased(alias) => (alias.value.as_str(), alias.is_named),
            ChildType::Normal(symbol) => (
                symbol_node_metadata(*symbol, ctx.syntax, ctx.lexical).0,
                variable_type == VariableType::Named,
            ),
        };
        let public = public_node_kind(name);
        if named {
            grammar.resolve_named_node(&public)
        } else {
            grammar.resolve_anonymous_node(&public)
        }
    } else {
        None
    };

    // Descent: the variable whose productions this step expands into. Derived from
    // the underlying symbol, not the alias, so an aliased non-terminal still points
    // at the body it renames. `None` for tokens (nothing to descend into).
    let body = step.symbol.is_non_terminal().then(|| {
        let index = u32::try_from(step.symbol.index).expect("syntax variable index fits in u32");
        VarId(index)
    });

    // A step the grammar calls visible must resolve to an id or a body — a visible
    // token has an id, a visible non-terminal has a body. A wholly-unknown visible
    // step means the classifier has a gap; debug builds (incl. the corpus test that
    // loads every grammar) catch it, release builds pay nothing.
    debug_assert!(
        !(variable_type.is_visible() && id.is_none() && body.is_none()),
        "visible step (symbol index {}) resolved to neither id nor body",
        step.symbol.index
    );

    SkeletonStep {
        target: StepTarget { id, body },
        field: step
            .field_name
            .as_deref()
            .and_then(|name| grammar.resolve_field(name)),
    }
}
