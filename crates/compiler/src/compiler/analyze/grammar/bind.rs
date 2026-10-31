use std::collections::{HashMap, HashSet};

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

pub struct GrammarBindInput<'a, 'q> {
    pub interner: &'a mut Interner,
    pub grammar: &'a Grammar,
    pub source_map: &'q SourceMap,
    pub definitions: &'a DefinitionGraph,
    pub pattern_facts: &'a PatternFacts,
    pub strict_lints: bool,
    pub satisfiability_limits: SatisfiabilityLimits,
    pub expression:
        Option<&'a crate::compiler::analyze::Located<crate::compiler::parse::ast::Pattern>>,
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
            expression,
        } = self;

        let mut node_kind_ids: HashMap<NodeKind<&'q str>, Option<NodeKindId>> = HashMap::new();
        let mut named_selectors: HashMap<String, Option<Vec<NodeKindId>>> = HashMap::new();
        let mut node_field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();
        let expression_calls = expression
            .into_iter()
            .flat_map(|body| definitions.expression_dependencies(body.node()))
            .collect();

        {
            let mut binder = GrammarBinder {
                interner: &mut *interner,
                grammar,
                source_map,
                definitions,
                expression_calls,
                strict_lints,
                node_kind_ids: &mut node_kind_ids,
                named_selectors: &mut named_selectors,
                node_field_ids: &mut node_field_ids,
                output,
                diag: diagnostics,
            };
            for definition_ids in definitions
                .ids_in_declaration_order()
                .chunk_by(|left, right| {
                    definitions.definition(*left).source()
                        == definitions.definition(*right).source()
                })
            {
                binder.bind(definition_ids);
            }
            if let Some(expression) = expression {
                binder.resolve_symbols(expression);
                if strict_lints {
                    binder.check_root_pattern(expression, "this expression");
                }
                binder.validate_syntax(expression);
            }
        }

        if diagnostics.has_errors() {
            return;
        }
        let model = super::model::Model::resolve(
            grammar,
            definitions,
            pattern_facts,
            &node_kind_ids,
            &named_selectors,
            &node_field_ids,
            expression,
        );
        super::satisfiability::check(
            super::satisfiability::ReportContext {
                grammar,
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
    expression_calls: HashSet<DefId>,
    pub(super) strict_lints: bool,
    pub(super) node_kind_ids: &'a mut HashMap<NodeKind<&'q str>, Option<NodeKindId>>,
    pub(super) named_selectors: &'a mut HashMap<String, Option<Vec<NodeKindId>>>,
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
                self.check_entrypoint_root(def_id);
            }
        }
        for &def_id in definition_ids {
            self.validate_syntax(&self.definitions.definition(def_id).located_body());
        }
    }

    fn check_entrypoint_root(&mut self, def_id: DefId) {
        if self.definitions.has_inbound_references(def_id)
            || self.expression_calls.contains(&def_id)
        {
            return;
        }

        let definition = self.definitions.definition(def_id);
        let located = definition.located_body();
        let name = self.interner.resolve(definition.name()).to_owned();
        self.check_root_pattern(&located, &format!("entrypoint `{name}`"));
    }

    fn check_root_pattern(
        &mut self,
        located: &crate::compiler::analyze::Located<crate::compiler::parse::ast::Pattern>,
        subject: &str,
    ) {
        let Some(grammar_root) = self.grammar.root() else {
            return;
        };
        let grammar_root_name = self
            .grammar
            .node_kind(grammar_root)
            .expect("grammar root must have a node-kind name");

        let mut seen_refs = std::collections::HashSet::new();
        if self.pattern_can_match_root(located, grammar_root, &mut seen_refs) {
            return;
        }

        self.diag
            .report(
                DiagnosticKind::EntrypointNeverMatchesRoot,
                Span::new(located.source(), located.node().text_range()),
            )
            .detail(format!(
                "{subject} cannot match the `{grammar_root_name}` syntax-tree root"
            ))
            .hint(format!("start with a `{grammar_root_name}` node pattern"))
            .emit();
    }
}
