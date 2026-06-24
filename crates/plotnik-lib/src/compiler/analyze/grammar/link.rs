//! Link pass: resolve node kinds and fields against tree-sitter grammar.
//!
//! Two-phase approach:
//! 1. Resolve all symbols (node kinds and fields) against grammar
//! 2. Validate structural constraints (field on node kind, child kind for field)

use std::collections::HashMap;

use crate::core::grammar::Grammar;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId};
use indexmap::IndexMap;

use super::check::{AdmissibilityMode, AdmissibilityWalkState};
use super::binding::GrammarBindingBuilder;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::diagnostics::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::Root;

/// The threaded dependencies of the link pass. Decoupled from `Query` to allow
/// testing without a full query context.
pub struct GrammarLinkInput<'a, 'q> {
    pub interner: &'a mut Interner,
    pub grammar: &'a Grammar,
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub symbol_table: &'a SymbolTable,
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
                node_kind_ids: &mut node_kind_ids,
                node_field_ids: &mut node_field_ids,
                output,
                diag: diagnostics,
            };
            linker.link(source_id, root);
        }
    }
}

pub(super) struct GrammarLinker<'a, 'q> {
    pub(super) interner: &'a mut Interner,
    pub(super) grammar: &'a Grammar,
    pub(super) source_map: &'q SourceMap,
    pub(super) symbol_table: &'a SymbolTable,
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
        self.check_grammar(source, root);
    }

    fn check_grammar(&mut self, source: SourceId, root: &Root) {
        let defs: Vec<_> = root.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            let located = Located::new(source, body);
            let mut walk = AdmissibilityWalkState::default();
            self.check_pattern_grammar(&located, None, AdmissibilityMode::Required, &mut walk);
        }
    }
}
