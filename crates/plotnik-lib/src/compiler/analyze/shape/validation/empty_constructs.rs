//! Semantic validation for empty constructs.
//!
//! Bans empty trees `()`, empty sequences `{}`, and empty alternations `[]`.

use super::ValidationInput;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{
    Visitor, walk_node_pattern, walk_seq_pattern, walk_union_pattern,
};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{NodePattern, SeqPattern, UnionPattern};

pub fn validate_empty_constructs(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut visitor = EmptyConstructsValidator { diag };
    visitor.visit(&Located::new(source_id, ast.clone()));
}

struct EmptyConstructsValidator<'d> {
    diag: &'d mut Diagnostics,
}

impl Visitor for EmptyConstructsValidator<'_> {
    fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
        // Check for truly empty tree: no child nodes at all in CST (only tokens like parens)
        // This excludes invalid content like predicates which create Error nodes
        if node.node().syntax().children().next().is_none() && node.node().kind_token().is_none() {
            self.diag
                .report(
                    DiagnosticKind::EmptyTree,
                    Span::new(node.source(), node.node().text_range()),
                )
                .emit();
        }
        walk_node_pattern(self, node);
    }

    fn visit_seq_pattern(&mut self, seq: &Located<SeqPattern>) {
        if seq.node().children().next().is_none() {
            self.diag
                .report(
                    DiagnosticKind::EmptySequence,
                    Span::new(seq.source(), seq.node().text_range()),
                )
                .emit();
        }
        walk_seq_pattern(self, seq);
    }

    fn visit_union_pattern(&mut self, union: &Located<UnionPattern>) {
        // An empty alternation `[]` has no labels, so it always casts to a union;
        // an enum always has at least one labeled branch.
        if union.node().branches().next().is_none() {
            self.diag
                .report(
                    DiagnosticKind::EmptyAlternation,
                    Span::new(union.source(), union.node().text_range()),
                )
                .emit();
        }
        walk_union_pattern(self, union);
    }
}
