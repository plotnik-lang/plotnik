//! Semantic validation for empty constructs.
//!
//! Bans empty trees `()`, empty sequences `{}`, and empty alternations `[]`.

use super::ValidationInput;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{
    Visitor, walk_alternation_pattern, walk_named_node_pattern, walk_seq_pattern,
};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::parse::ast::{AlternationPattern, NamedNodePattern, SeqPattern};
use crate::compiler::parse::cst::SyntaxNode;

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
    fn visit_named_node_pattern(&mut self, node: &Located<NamedNodePattern>) {
        if is_construct_empty(node.node().syntax()) && node.node().kind_token().is_none() {
            self.diag
                .report(
                    DiagnosticKind::EmptyTree,
                    node.span_of(node.node().text_range()),
                )
                .emit();
        }
        walk_named_node_pattern(self, node);
    }

    fn visit_seq_pattern(&mut self, seq: &Located<SeqPattern>) {
        if is_construct_empty(seq.node().syntax()) {
            self.diag
                .report(
                    DiagnosticKind::EmptySequence,
                    seq.span_of(seq.node().text_range()),
                )
                .emit();
        }
        walk_seq_pattern(self, seq);
    }

    fn visit_alternation_pattern(&mut self, alternation: &Located<AlternationPattern>) {
        if is_construct_empty(alternation.node().syntax()) {
            self.diag
                .report(
                    DiagnosticKind::EmptyAlternation,
                    alternation.span_of(alternation.node().text_range()),
                )
                .emit();
        }
        walk_alternation_pattern(self, alternation);
    }
}

fn is_construct_empty(syntax: &SyntaxNode) -> bool {
    // Invalid contents still produce error nodes, so only delimiter-only constructs count.
    syntax.children().next().is_none()
}
