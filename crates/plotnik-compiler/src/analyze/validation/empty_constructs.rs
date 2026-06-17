//! Semantic validation for empty constructs.
//!
//! Bans empty trees `()`, empty sequences `{}`, and empty alternations `[]`.

use super::ValidateInput;
use crate::analyze::Reporter;
use crate::analyze::visitor::{Visitor, walk_alt_expr, walk_named_node, walk_seq_expr};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AltExpr, NamedNode, SeqExpr};

pub fn validate_empty_constructs(input: ValidateInput) {
    let ValidateInput {
        source_id,
        ast,
        diag,
        ..
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
    fn visit_named_node(&mut self, node: &NamedNode) {
        // Check for truly empty tree: no child nodes at all in CST (only tokens like parens)
        // This excludes invalid content like predicates which create Error nodes
        if node.as_cst().children().next().is_none() && node.node_type().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptyTree, node.text_range())
                .emit();
        }
        walk_named_node(self, node);
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        if seq.children().next().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptySequence, seq.text_range())
                .emit();
        }
        walk_seq_expr(self, seq);
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        if alt.branches().next().is_none() {
            self.reporter
                .report(DiagnosticKind::EmptyAlternation, alt.text_range())
                .emit();
        }
        walk_alt_expr(self, alt);
    }
}
