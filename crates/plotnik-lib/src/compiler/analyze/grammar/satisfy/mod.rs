//! Stage B: sequence / anchor / arity satisfiability.
//!
//! Stage A (`../check.rs`) validates each query position in isolation — kind exists,
//! field is on the node, child kind is admissible. It is order-, adjacency-, and
//! arity-blind, so `(function_declaration .! (identifier))` and `(array (statement))`
//! slip through. This pass closes that gap: it threads the grammar's productions
//! through a per-query-node child automaton (`automaton.rs`) under a least fixed
//! point (`engine.rs`), and rejects a query node exactly when no tree the grammar can
//! produce realizes its children. `diagnose.rs` turns a failure into a message that
//! points at the deepest culprit and explains the obstacle.
//!
//! The contract is **sound rejection**: every rejection is truly impossible; whenever
//! the question cannot be decided, the pass accepts. A false rejection is the one
//! unacceptable outcome, so the walk only reports at `Required` positions — a
//! concrete-kind node not under an alternation branch or quantified body, where a
//! failure cannot be excused by a sibling branch or zero repetitions.

mod automaton;
mod diagnose;
mod engine;
mod state_set;

#[cfg(test)]
mod state_set_tests;

use indexmap::IndexMap;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::{NodePattern, Pattern, Root, token_src};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::NodeKindId;
use crate::core::grammar::Grammar;

use automaton::AutomatonContext;
use engine::Satisfier;

/// The threaded dependencies of the satisfiability pass.
pub(super) struct SatisfyInput<'a> {
    pub(super) grammar: &'a Grammar,
    pub(super) symbol_table: &'a SymbolTable,
    pub(super) source_map: &'a SourceMap,
    pub(super) ast_map: &'a IndexMap<SourceId, Root>,
}

/// Run the satisfiability pass over every definition, reporting impossible patterns.
pub(super) fn check(input: SatisfyInput<'_>, diag: &mut Diagnostics) {
    // A metadata-only grammar has no retained productions, so nothing can be decided;
    // accept everything rather than reason from an empty model.
    if input.grammar.structure().variables().is_empty() {
        return;
    }

    let ctx = AutomatonContext {
        grammar: input.grammar,
        symbol_table: input.symbol_table,
        source_map: input.source_map,
    };

    // A definition is matchable in its own right, so each body roots a Required goal
    // (the same stance Stage A takes). References are not followed here: a node used
    // as a whole child is judged by the engine in context, and a definition reached
    // through a reference is itself walked when the loop reaches its own definition.
    let mut goals = Vec::new();
    for (&source, root) in input.ast_map {
        for def in root.defs() {
            let Some(body) = def.body() else { continue };
            collect_goals(&Located::new(source, body), Mode::Required, ctx, &mut goals);
        }
    }

    let mut satisfier = Satisfier::new(ctx, false);
    for goal in &goals {
        if !satisfier.satisfiable(&goal.node, goal.kind) {
            diagnose::report(&mut satisfier, &goal.node, goal.kind, diag);
        }
    }
}

/// A concrete-kind node pattern that must match, paired with its resolved kind.
struct Goal {
    node: Located<NodePattern>,
    kind: NodeKindId,
}

/// Whether a position must participate in every match. A position turns `Deferred`
/// once the walk descends into an alternation branch or a `?`/`*` body — there a
/// failure is excused, so reporting it would reject a query that can match. A `+`
/// body keeps the mode: it must match at least once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Required,
    Deferred,
}

impl Mode {
    fn is_required(self) -> bool {
        matches!(self, Mode::Required)
    }
}

/// Collect the concrete-kind node patterns at `Required` positions reachable from
/// `located` without crossing into another node's child list. The walk descends
/// through the always-present wrappers (capture, field, sequence) and the
/// disjunctive ones (alternation, quantifier, lowering them to `Deferred`), but
/// stops at each node pattern — its subtree is judged whole by the engine.
fn collect_goals(
    located: &Located<Pattern>,
    mode: Mode,
    ctx: AutomatonContext<'_>,
    out: &mut Vec<Goal>,
) {
    match located.node() {
        Pattern::NodePattern(node) => {
            if !mode.is_required() {
                return;
            }
            let located_node = located.wrap(node.clone());
            if let Some(kind) = root_kind(ctx, &located_node) {
                out.push(Goal {
                    node: located_node,
                    kind,
                });
            }
        }
        // An anonymous literal at a goal position matches some token of the grammar.
        Pattern::TokenPattern(_) => {}
        Pattern::CapturedPattern(cap) => {
            if let Some(inner) = cap.inner() {
                collect_goals(&located.wrap(inner), mode, ctx, out);
            }
        }
        Pattern::FieldPattern(field) => {
            if let Some(value) = field.value() {
                collect_goals(&located.wrap(value), mode, ctx, out);
            }
        }
        Pattern::SeqPattern(seq) => {
            for child in seq.children() {
                collect_goals(&located.wrap(child), mode, ctx, out);
            }
        }
        Pattern::QuantifiedPattern(q) => {
            let Some(inner) = q.inner() else { return };
            // `?`/`*` admit zero matches, so the body need not hold; `+` needs it once.
            let inner_mode = if q.is_optional() { Mode::Deferred } else { mode };
            collect_goals(&located.wrap(inner), inner_mode, ctx, out);
        }
        Pattern::Union(_) | Pattern::Enum(_) => {
            for branch in located.node().children() {
                collect_goals(&located.wrap(branch), Mode::Deferred, ctx, out);
            }
        }
        // A reference's target is walked when the loop reaches its own definition.
        Pattern::DefRef(_) => {}
    }
}

/// The concrete named kind a node pattern roots a goal at, or `None` when the
/// position should be skipped (wildcard, supertype, error keyword, unresolved).
fn root_kind(ctx: AutomatonContext<'_>, located: &Located<NodePattern>) -> Option<NodeKindId> {
    let node = located.node();
    if node.is_any() {
        return None;
    }
    let type_token = node.kind_token()?;
    if matches!(type_token.kind(), SyntaxKind::KwError | SyntaxKind::KwMissing) {
        return None;
    }
    let text = token_src(&type_token, ctx.content(located.source()));
    let id = ctx.grammar.resolve_named_node(text)?;
    // Query supertypes are rejected by Stage A; if one reaches here, skip it.
    if ctx.grammar.is_supertype(id) {
        return None;
    }
    Some(id)
}
