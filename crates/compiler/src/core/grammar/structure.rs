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
    pub(crate) fn visible_kind(self, grammar: &Grammar) -> Option<NodeKindId> {
        self.id.filter(|&id| !grammar.is_supertype(id))
    }

    pub(crate) fn transparent_body(self, grammar: &Grammar) -> Option<VarId> {
        match (self.id, self.body) {
            (Some(id), Some(body)) if grammar.is_supertype(id) => Some(body),
            (None, Some(body)) => Some(body),
            _ => None,
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepProjection {
    Visible {
        kind: NodeKindId,
        field: Option<NodeFieldId>,
        body: Option<VarId>,
    },
    Transparent {
        body: VarId,
        field: Option<NodeFieldId>,
    },
    HiddenLeaf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissibilityStep {
    Field {
        field: NodeFieldId,
        value: FieldValueProjection,
    },
    Child {
        kind: NodeKindId,
        transparent_fields: Option<VarId>,
    },
    Transparent {
        body: VarId,
    },
    HiddenLeaf,
}

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

        let tokens = (0..ctx.lexical.variables.len())
            .map(Symbol::terminal)
            .chain(
                ctx.syntax
                    .external_tokens
                    .iter()
                    .enumerate()
                    .filter(|(_, token)| token.corresponding_internal_token.is_none())
                    .map(|(index, _)| Symbol::external(index)),
            );
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
    let child_type = alias
        .cloned()
        .map_or(ChildType::Normal(symbol), ChildType::Aliased);

    let variable_type = variable_type_for_child_type(&child_type, ctx.syntax, ctx.lexical);

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
