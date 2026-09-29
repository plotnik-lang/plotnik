//! Distilled, retained structural skeleton of the flattened grammar.
//!
//! `node_shapes` summarizes *what can be inside what* and, in doing so, discards
//! order and adjacency. This module retains what that summary throws away: the
//! ordered production step sequences. Each step is resolved into two independent
//! facts — *what it matches* (a public id) and *what it descends into* (a
//! variable) — built from the same [`GrammarContext`] and reusing `node_shapes`'
//! symbol resolution and `lower`'s public-name normalization so the views cannot
//! drift. It is the substrate for sequence/anchor satisfiability and first-set
//! analysis.
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
//!   Classification fails grammar construction if a visible token has no public id.
//! - **Built eagerly by `from_raw`.** Productions and token realizations are retained
//!   before the prepared grammar is dropped. Every constructed grammar has this
//!   table, shared by reachability, satisfiability, and token classification.

use std::collections::HashMap;

use crate::core::{NodeFieldId, NodeKindId};

use super::lower::public_node_kind;
use super::node_shapes::{
    ChildType, GrammarContext, effective_step_alias, symbol_node_metadata,
    variable_type_for_child_type,
};
use super::prepared::{ProductionStep, VariableType};
use super::rules::{Alias, Symbol};
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

impl StepTarget {
    /// Public concrete kind this step surfaces as a child. Supertypes are not concrete
    /// CST nodes, so consumers that need runtime-visible steps must descend through them.
    pub(crate) fn visible_kind(self, grammar: &Grammar) -> Option<NodeKindId> {
        self.id.filter(|&id| !grammar.is_supertype(id))
    }

    /// Variable whose children/fields surface through this step without crossing into
    /// a concrete child: id-less inlined rules and kept supertypes.
    pub(crate) fn transparent_body(self, grammar: &Grammar) -> Option<VarId> {
        match (self.id, self.body) {
            (Some(id), Some(body)) if grammar.is_supertype(id) => Some(body),
            (None, Some(body)) => Some(body),
            _ => None,
        }
    }

    /// Id-less field values stand for the frontier of their body. A target with an id
    /// is already the value kind to record, even if it also descends into a body.
    pub(crate) fn idless_value_body(self) -> Option<VarId> {
        if self.id.is_none() { self.body } else { None }
    }
}

/// One production step: the node it matches, and the field it binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkeletonStep {
    pub target: StepTarget,
    /// Field this step binds to, if any.
    pub field: Option<NodeFieldId>,
}

impl SkeletonStep {
    pub(crate) fn projection(self, grammar: &Grammar) -> StepProjection {
        if let Some(kind) = self.target.visible_kind(grammar) {
            return StepProjection::Visible {
                kind,
                field: self.field,
                body: self.target.body,
            };
        }
        if let Some(body) = self.target.transparent_body(grammar) {
            StepProjection::Transparent {
                body,
                field: self.field,
            }
        } else {
            StepProjection::HiddenLeaf
        }
    }

    pub(crate) fn admissibility(self, grammar: &Grammar) -> AdmissibilityStep {
        if let Some(field) = self.field {
            return AdmissibilityStep::Field {
                field,
                value: self.field_value(),
            };
        }
        if let Some(kind) = self.target.id {
            return AdmissibilityStep::Child {
                kind,
                transparent_fields: self.target.transparent_body(grammar),
            };
        }
        if let Some(body) = self.target.transparent_body(grammar) {
            AdmissibilityStep::Transparent { body }
        } else {
            AdmissibilityStep::HiddenLeaf
        }
    }

    pub(crate) fn field_value(self) -> FieldValueProjection {
        if let Some(kind) = self.target.id {
            return FieldValueProjection::Kind(kind);
        }
        if let Some(body) = self.target.idless_value_body() {
            FieldValueProjection::Frontier(body)
        } else {
            FieldValueProjection::Empty
        }
    }
}

/// How a skeleton step surfaces to consumers that reason over visible tree children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepProjection {
    Visible {
        kind: NodeKindId,
        field: Option<NodeFieldId>,
        /// Variable the visible child descends into, if it is not a token.
        body: Option<VarId>,
    },
    Transparent {
        body: VarId,
        /// Field pushed onto the transparent frontier, if any.
        field: Option<NodeFieldId>,
    },
    HiddenLeaf,
}

/// How a skeleton step widens the structural admissibility model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissibilityStep {
    Field {
        field: NodeFieldId,
        value: FieldValueProjection,
    },
    Child {
        kind: NodeKindId,
        /// Supertype members may expose fields on the enclosing node.
        transparent_fields: Option<VarId>,
    },
    Transparent {
        body: VarId,
    },
    HiddenLeaf,
}

/// The value side of a fielded step or an id-less value frontier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldValueProjection {
    Kind(NodeKindId),
    Frontier(VarId),
    Empty,
}

/// A grammar variable reduced to its productions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkeletonVariable {
    /// Rule name for diagnostics, including rules hidden behind aliases.
    pub name: String,
    /// Public identity after applying the variable's default alias. `None` for
    /// hidden variables. Individual production occurrences can use other aliases.
    pub id: Option<NodeKindId>,
    pub kind: VariableType,
    /// One inner `Vec` per production, each an ordered list of steps.
    pub productions: Vec<Vec<SkeletonStep>>,
}

/// The child structure behind one occurrence of a public node kind. An alias can
/// give the same kind to both a token and a syntax variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeRealizer {
    Leaf,
    Var(VarId),
}

impl NodeRealizer {
    pub(crate) fn of_body(body: Option<VarId>) -> Self {
        match body {
            Some(body) => Self::Var(body),
            None => Self::Leaf,
        }
    }
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
    realizers: HashMap<NodeKindId, Vec<NodeRealizer>>,
}

impl StructureTable {
    pub(super) fn build(ctx: GrammarContext<'_>, grammar: &Grammar) -> Result<Self, String> {
        let variables = ctx
            .syntax
            .variables
            .iter()
            .enumerate()
            .map(|(index, variable)| {
                let symbol = Symbol::non_terminal(index);
                let target = classify_target(symbol, ctx.aliases.get(&symbol), ctx, grammar)?;
                let name = public_node_kind(&variable.name);
                Ok(SkeletonVariable {
                    id: target.id,
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
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut table = Self {
            variables,
            realizers: HashMap::new(),
        };
        table.index_realizers(ctx, grammar)?;
        Ok(table)
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

    pub(crate) fn realizers_of(&self, kind: NodeKindId) -> &[NodeRealizer] {
        self.realizers
            .get(&kind)
            .expect("every queryable concrete kind has a retained realization")
    }

    pub(crate) fn realizers(&self) -> &HashMap<NodeKindId, Vec<NodeRealizer>> {
        &self.realizers
    }

    fn index_realizers(
        &mut self,
        ctx: GrammarContext<'_>,
        grammar: &Grammar,
    ) -> Result<(), String> {
        let mut insert = |target: StepTarget| {
            let Some(kind) = target.id else {
                return;
            };

            let realizer = NodeRealizer::of_body(target.body);
            let entry = self.realizers.entry(kind).or_default();
            if !entry.contains(&realizer) {
                entry.push(realizer);
            }
        };
        for (index, variable) in self.variables.iter().enumerate() {
            insert(StepTarget {
                id: variable.id,
                body: Some(VarId(index as u32)),
            });
            for step in variable.productions.iter().flatten() {
                insert(step.target);
            }
        }

        // Tokens used only as extras have no production occurrence. Include their
        // public identities, along with external tokens and default token aliases.
        let tokens = (0..ctx.lexical.variables.len())
            .map(Symbol::terminal)
            .chain((0..ctx.syntax.external_tokens.len()).map(Symbol::external));
        for symbol in tokens {
            insert(classify_target(
                symbol,
                ctx.aliases.get(&symbol),
                ctx,
                grammar,
            )?);
        }

        let kinds = grammar
            .all_named_node_kinds()
            .into_iter()
            .map(|name| {
                grammar
                    .resolve_named_node(name)
                    .expect("listed named kind resolves")
            })
            .chain(grammar.all_anonymous_node_kinds().into_iter().map(|name| {
                grammar
                    .resolve_anonymous_node(name)
                    .expect("listed anonymous kind resolves")
            }));
        for kind in kinds {
            if !grammar.is_supertype(kind) && !self.realizers.contains_key(&kind) {
                let name = grammar.node_kind(kind).expect("resolved kind has a name");
                return Err(format!("node kind `{name}` has no retained realization"));
            }
        }

        Ok(())
    }
}

fn classify_step(
    step: &ProductionStep,
    ctx: GrammarContext<'_>,
    grammar: &Grammar,
) -> Result<SkeletonStep, String> {
    Ok(SkeletonStep {
        target: classify_target(
            step.symbol,
            effective_step_alias(step, ctx.aliases),
            ctx,
            grammar,
        )?,
        field: step
            .field_name
            .as_deref()
            .and_then(|name| grammar.resolve_field(name)),
    })
}

fn classify_target(
    symbol: Symbol,
    alias: Option<&Alias>,
    ctx: GrammarContext<'_>,
    grammar: &Grammar,
) -> Result<StepTarget, String> {
    // Use the effective alias supplied by the occurrence, including default aliases,
    // so production steps and standalone tokens share the node-shape classification.
    let child_type = alias
        .cloned()
        .map_or(ChildType::Normal(symbol), ChildType::Aliased);

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
    let body = symbol.is_non_terminal().then(|| {
        let index = u32::try_from(symbol.index).expect("syntax variable index fits in u32");
        VarId(index)
    });

    if variable_type.is_visible() && id.is_none() && body.is_none() {
        return Err(format!(
            "visible production step {} resolves to neither a public node kind nor a rule body",
            symbol.index
        ));
    }

    Ok(StepTarget { id, body })
}
