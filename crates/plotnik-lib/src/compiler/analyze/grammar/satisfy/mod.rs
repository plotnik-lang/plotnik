//! Stage B: sequence / anchor / arity satisfiability.
//!
//! Stage A (`../check.rs`) validates each query position in isolation — kind exists,
//! field is on the node, child kind is admissible. It is order-, adjacency-, and
//! arity-blind, so `(function_declaration .! (identifier))` and `(array (statement))`
//! slip through. This pass closes that gap: it threads the grammar's productions
//! through a per-query-node child automaton (`automaton.rs`) under a least fixed
//! point (`engine.rs`), and rejects a query node exactly when no tree the grammar can
//! produce realizes its children.
//!
//! The contract is **sound rejection**: every rejection is truly impossible; whenever
//! the question cannot be decided, the pass accepts. A false rejection is the one
//! unacceptable outcome, so the walk only reports at `Required` positions — a
//! concrete-kind node not under an alternation branch or quantified body, where a
//! failure cannot be excused by a sibling branch or zero repetitions.

mod automaton;
mod engine;
mod state_set;

#[cfg(test)]
mod state_set_tests;

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
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
    let mut satisfier = Satisfier::new(ctx, false);
    let mut pass = Pass {
        ctx,
        satisfier: &mut satisfier,
        diag,
        visiting: HashSet::new(),
    };

    for (&source, root) in input.ast_map {
        for def in root.defs() {
            let Some(body) = def.body() else { continue };
            pass.walk(&Located::new(source, body), Mode::Required);
        }
    }
}

/// Whether a position must participate in every match. A position turns `Deferred`
/// once the walk descends into an alternation branch or a quantified body — there a
/// failure is excused, so reporting it would reject a query that can match.
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

struct Pass<'a, 's, 'd> {
    ctx: AutomatonContext<'a>,
    satisfier: &'s mut Satisfier<'a>,
    diag: &'d mut Diagnostics,
    /// Definition names on the current goal-walk stack, to break reference cycles.
    visiting: HashSet<String>,
}

impl Pass<'_, '_, '_> {
    /// Walk a pattern, returning whether it is satisfiable and reporting an impossible
    /// concrete node at a `Required` position. The walk stops at each node pattern —
    /// its whole subtree is judged by the engine, not re-walked here.
    fn walk(&mut self, located: &Located<Pattern>, mode: Mode) -> bool {
        match located.node() {
            Pattern::NodePattern(node) => {
                let located_node = located.wrap(node.clone());
                let satisfiable = self.node_satisfiable(&located_node);
                if !satisfiable && mode.is_required() {
                    self.report(&located_node);
                }
                satisfiable
            }
            // An anonymous literal at a goal position matches some token of the
            // grammar; structural impossibility never originates here.
            Pattern::TokenPattern(_) => true,
            Pattern::CapturedPattern(cap) => match cap.inner() {
                Some(inner) => self.walk(&located.wrap(inner), mode),
                None => true,
            },
            Pattern::FieldPattern(field) => match field.value() {
                Some(value) => self.walk(&located.wrap(value), mode),
                None => true,
            },
            Pattern::SeqPattern(seq) => {
                // Siblings of a sequence all participate, so each keeps the mode.
                let mut satisfiable = true;
                for child in seq.children() {
                    satisfiable &= self.walk(&located.wrap(child), mode);
                }
                satisfiable
            }
            Pattern::QuantifiedPattern(q) => {
                let Some(inner) = q.inner() else {
                    return true;
                };
                // `?`/`*` admit zero matches, so the body is deferred and the whole is
                // satisfiable regardless; `+` needs the body once, so it stays in mode.
                if q.is_optional() {
                    self.walk(&located.wrap(inner), Mode::Deferred);
                    true
                } else {
                    self.walk(&located.wrap(inner), mode)
                }
            }
            Pattern::Union(_) | Pattern::Enum(_) => {
                // A branch is excused by its siblings, so each is deferred; the
                // alternation is satisfiable if any branch is. Reporting the
                // all-branches-impossible case is left to a later graft.
                let mut any = false;
                for branch in located.node().children() {
                    any |= self.walk(&located.wrap(branch), Mode::Deferred);
                }
                any
            }
            Pattern::DefRef(def_ref) => {
                let Some(name_token) = def_ref.name() else {
                    return true;
                };
                let name = name_token.text();
                // A reference cycle in the goal walk is judged by the engine's fixed
                // point, not here; cut it and accept.
                if !self.visiting.insert(name.to_string()) {
                    return true;
                }
                let result = match self.ctx.symbol_table.located_definition(name) {
                    Some(target) => self.walk(&target, mode),
                    None => true,
                };
                self.visiting.remove(name);
                result
            }
        }
    }

    fn node_satisfiable(&mut self, located: &Located<NodePattern>) -> bool {
        match self.root_kind(located) {
            Some(kind) => self.satisfier.satisfiable(located, kind),
            // Wildcards, supertypes (pre-rejected), and `ERROR`/`MISSING` carry no
            // decidable structural goal — accept.
            None => true,
        }
    }

    /// The concrete named kind a node pattern roots a goal at, or `None` when the
    /// position should be skipped (wildcard, supertype, error keyword, unresolved).
    fn root_kind(&self, located: &Located<NodePattern>) -> Option<NodeKindId> {
        let node = located.node();
        if node.is_any() {
            return None;
        }
        let type_token = node.kind_token()?;
        if matches!(type_token.kind(), SyntaxKind::KwError | SyntaxKind::KwMissing) {
            return None;
        }
        let text = token_src(&type_token, self.ctx.content(located.source()));
        let id = self.ctx.grammar.resolve_named_node(text)?;
        // Query supertypes are rejected by Stage A; if one reaches here, skip it.
        if self.ctx.grammar.is_supertype(id) {
            return None;
        }
        Some(id)
    }

    fn report(&mut self, located: &Located<NodePattern>) {
        let span = located
            .node()
            .kind_token()
            .map(|token| located.span_of(token.text_range()))
            .unwrap_or_else(|| located.span_of(located.node().text_range()));
        self.diag
            .report(DiagnosticKind::UnsatisfiablePattern, span)
            .emit();
    }
}
