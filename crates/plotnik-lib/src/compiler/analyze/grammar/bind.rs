//! Bind pass: resolve node kinds and fields against a tree-sitter grammar.
//!
//! Two-phase approach:
//! 1. Resolve all symbols (node kinds and fields) against grammar
//! 2. Validate syntax, then solve the resolved matching constraints

use std::collections::HashMap;

use crate::core::grammar::Grammar;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId};

use super::binding::GrammarBindingBuilder;
use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::diagnostics::Span;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::ids::DefId;
use crate::compiler::limits::SatisfiabilityLimits;

/// The threaded dependencies of the bind pass. Decoupled from `Query` to allow
/// testing without a full query context.
pub struct GrammarBindInput<'a, 'q> {
    pub interner: &'a mut Interner,
    pub grammar: &'a Grammar,
    pub source_map: &'q SourceMap,
    pub definitions: &'a DefinitionGraph,
    pub pattern_facts: &'a PatternFacts,
    pub strict_lints: bool,
    pub satisfiability_limits: SatisfiabilityLimits,
}

impl<'q> GrammarBindInput<'_, 'q> {
    pub(crate) fn bind(self, output: &mut GrammarBindingBuilder, diagnostics: &mut Diagnostics) {
        let GrammarBindInput {
            interner,
            grammar,
            source_map,
            definitions,
            pattern_facts,
            strict_lints,
            satisfiability_limits,
        } = self;

        let mut node_kind_ids: HashMap<NodeKind<&'q str>, Option<NodeKindId>> = HashMap::new();
        let mut node_field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();

        // Resolve, lint, and check all definitions from one source before moving
        // to the next, preserving the bind pass's diagnostic phase order.
        for definition_ids in definitions
            .ids_in_declaration_order()
            .chunk_by(|left, right| {
                definitions.definition(*left).source() == definitions.definition(*right).source()
            })
        {
            let mut binder = GrammarBinder {
                interner: &mut *interner,
                grammar,
                source_map,
                definitions,
                strict_lints,
                node_kind_ids: &mut node_kind_ids,
                node_field_ids: &mut node_field_ids,
                output,
                diag: diagnostics,
            };
            binder.bind(definition_ids);
        }

        // Invalid names and unsupported syntax are unconditional errors. Matching
        // constraints are evaluated only after every definition passes this boundary.
        if diagnostics.has_errors() {
            return;
        }
        let model = super::model::Model::resolve(
            grammar,
            definitions,
            pattern_facts,
            source_map,
            &node_kind_ids,
            &node_field_ids,
        );
        super::satisfiability::check(
            super::satisfiability::ReportContext {
                grammar,
                pattern_facts,
                source_map,
                model: &model,
            },
            satisfiability_limits,
            diagnostics,
        );
    }
}

pub(super) struct GrammarBinder<'a, 'q> {
    pub(super) interner: &'a mut Interner,
    pub(super) grammar: &'a Grammar,
    pub(super) source_map: &'q SourceMap,
    pub(super) definitions: &'a DefinitionGraph,
    pub(super) strict_lints: bool,
    pub(super) node_kind_ids: &'a mut HashMap<NodeKind<&'q str>, Option<NodeKindId>>,
    pub(super) node_field_ids: &'a mut HashMap<&'q str, Option<NodeFieldId>>,
    pub(super) output: &'a mut GrammarBindingBuilder,
    pub(super) diag: &'a mut Diagnostics,
}

impl<'a, 'q> GrammarBinder<'a, 'q> {
    pub(super) fn content(&self, source: SourceId) -> &'q str {
        self.source_map.content(source)
    }

    fn bind(&mut self, definition_ids: &[DefId]) {
        for &def_id in definition_ids {
            let body = self.definitions.definition(def_id).located_body();
            self.resolve_symbols(&body);
        }
        if self.strict_lints {
            for &def_id in definition_ids {
                self.check_entry_point_root(def_id);
            }
        }
        for &def_id in definition_ids {
            self.validate_syntax(&self.definitions.definition(def_id).located_body());
        }
    }

    fn check_entry_point_root(&mut self, def_id: DefId) {
        let Some(grammar_root) = self.grammar.root() else {
            return;
        };
        let grammar_root_name = self
            .grammar
            .node_kind(grammar_root)
            .expect("grammar root must have a node-kind name");

        if self.definitions.has_inbound_references(def_id) {
            return;
        }
        let definition = self.definitions.definition(def_id);
        let located = definition.located_body();
        let mut seen_refs = std::collections::HashSet::new();
        if self.pattern_can_match_root(&located, grammar_root, &mut seen_refs) {
            return;
        }
        let name = self.interner.resolve(definition.name());

        self.diag
            .report(
                DiagnosticKind::EntryPointNeverMatchesRoot,
                Span::new(located.source(), located.node().text_range()),
            )
            .detail(format!(
                "entry point `{name}` cannot match the `{grammar_root_name}` syntax-tree root"
            ))
            .hint(format!(
                "make `{name}` start with a `{grammar_root_name}` node pattern because matching begins at the syntax-tree root"
            ))
            .emit();
    }
}
