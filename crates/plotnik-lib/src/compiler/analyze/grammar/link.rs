//! Link pass: resolve node kinds and fields against tree-sitter grammar.
//!
//! Two-phase approach:
//! 1. Resolve all symbols (node kinds and fields) against grammar
//! 2. Validate structural constraints (field on node kind, child kind for field)

use std::collections::HashMap;

use crate::core::grammar::Grammar;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId};
use indexmap::IndexMap;

use super::binding::GrammarBindingBuilder;
use super::check::AdmissibilityWalkState;
use super::participation::Participation;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::diagnostics::Span;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::Root;

/// The threaded dependencies of the link pass. Decoupled from `Query` to allow
/// testing without a full query context.
pub struct GrammarLinkInput<'a, 'q> {
    pub interner: &'a mut Interner,
    pub grammar: &'a Grammar,
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub strict_lints: bool,
    pub satisfiability_limits: SatisfiabilityLimits,
}

impl<'q> GrammarLinkInput<'_, 'q> {
    pub(crate) fn link(self, output: &mut GrammarBindingBuilder, diagnostics: &mut Diagnostics) {
        // Local deduplication maps (not exposed in output)
        let mut node_kind_ids: HashMap<NodeKind<&'q str>, Option<NodeKindId>> = HashMap::new();
        let mut node_field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();

        for (&source_id, root) in self.ast_map {
            let mut linker = GrammarLinker {
                interner: &mut *self.interner,
                grammar: self.grammar,
                source_map: self.source_map,
                symbol_table: self.symbol_table,
                dependency_analysis: self.dependency_analysis,
                strict_lints: self.strict_lints,
                node_kind_ids: &mut node_kind_ids,
                node_field_ids: &mut node_field_ids,
                output,
                diag: diagnostics,
            };
            linker.link(source_id, root);
        }

        // The satisfiability check (sequence/anchor/arity) runs only on a query the
        // structural check left clean: it adds rejections those checks cannot see, and
        // gating keeps it from piling onto an impossibility already pinned precisely.
        if diagnostics.has_errors() {
            return;
        }
        super::satisfiability::check(
            super::satisfiability::SatisfiabilityInput {
                grammar: self.grammar,
                symbol_table: self.symbol_table,
                source_map: self.source_map,
                ast_map: self.ast_map,
                limits: self.satisfiability_limits,
            },
            diagnostics,
        );
    }
}

pub(super) struct GrammarLinker<'a, 'q> {
    pub(super) interner: &'a mut Interner,
    pub(super) grammar: &'a Grammar,
    pub(super) source_map: &'q SourceMap,
    pub(super) symbol_table: &'a SymbolTable,
    pub(super) dependency_analysis: &'a DependencyAnalysis,
    pub(super) strict_lints: bool,
    pub(super) node_kind_ids: &'a mut HashMap<NodeKind<&'q str>, Option<NodeKindId>>,
    pub(super) node_field_ids: &'a mut HashMap<&'q str, Option<NodeFieldId>>,
    pub(super) output: &'a mut GrammarBindingBuilder,
    pub(super) diag: &'a mut Diagnostics,
}

impl<'a, 'q> GrammarLinker<'a, 'q> {
    pub(super) fn content(&self, source: SourceId) -> &'q str {
        self.source_map.content(source)
    }

    fn link(&mut self, source: SourceId, root: &Root) {
        self.resolve_symbols(source, root);
        if self.strict_lints {
            self.check_entrypoint_roots(source, root);
        }
        self.check_grammar(source, root);
    }

    fn check_entrypoint_roots(&mut self, source: SourceId, root: &Root) {
        let Some(grammar_root) = self.grammar.root() else {
            return;
        };

        for def in root.defs() {
            let Some(name) = def.name() else { continue };
            let Some(sym) = self.interner.get(name.text()) else {
                continue;
            };
            let Some(def_id) = self.dependency_analysis.def_id_for_sym(sym) else {
                continue;
            };
            if self.dependency_analysis.has_inbound_references(def_id) {
                continue;
            }
            let Some(body) = def.body() else { continue };
            let located = Located::new(source, body);
            let mut seen_refs = std::collections::HashSet::new();
            if self.pattern_can_match_root(&located, grammar_root, &mut seen_refs) {
                continue;
            }

            self.diag
                .report(
                    DiagnosticKind::EntrypointNeverMatchesRoot,
                    Span::new(source, located.node().text_range()),
                )
                .emit();
        }
    }

    fn check_grammar(&mut self, source: SourceId, root: &Root) {
        let defs: Vec<_> = root.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            let located = Located::new(source, body);
            let mut walk = AdmissibilityWalkState::default();
            self.check_pattern_grammar(&located, None, Participation::Required, &mut walk);
        }
    }
}
