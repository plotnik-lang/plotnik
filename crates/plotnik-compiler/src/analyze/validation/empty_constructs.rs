//! Semantic validation for empty constructs.
//!
//! Bans empty trees `()`, empty sequences `{}`, and empty alternations `[]`.

use super::ValidationInput;
use crate::analyze::Reporter;
use crate::analyze::visitor::{Visitor, walk_alt_pattern, walk_node_pattern, walk_seq_pattern};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AltPattern, NodePattern, SeqPattern};

pub fn validate_empty_constructs(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut visitor = EmptyConstructsValidator {
        reporter: Reporter::new(source_id, diag),
    };
    visitor.visit(ast);
}

struct EmptyConstructsValidator<'a> {
    reporter: Reporter<'a>,
}

impl Visitor for EmptyConstructsValidator<'_> {
    fn visit_node_pattern(&mut self, node: &NodePattern) {
        // Check for truly empty tree: no child nodes at all in CST (only tokens like parens)
        // This excludes invalid content like predicates which create Error nodes
        if node.syntax().children().next().is_none() && node.kind_token().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptyTree, node.text_range())
                .emit();
        }
        walk_node_pattern(self, node);
    }

    fn visit_seq_pattern(&mut self, seq: &SeqPattern) {
        if seq.children().next().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptySequence, seq.text_range())
                .emit();
        }
        walk_seq_pattern(self, seq);
    }

    fn visit_alt_pattern(&mut self, alt: &AltPattern) {
        if alt.branches().next().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptyAlternation, alt.text_range())
                .emit();
        }
        walk_alt_pattern(self, alt);
    }
}
