//! Semantic validation for empty constructs.
//!
//! Bans empty trees `()`, empty sequences `{}`, and empty alternations `[]`.

use crate::SourceId;
use crate::analyze::visitor::{Visitor, walk_alt_expr, walk_named_node, walk_seq_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{AltExpr, NamedNode, Root, SeqExpr};

pub fn validate_empty_constructs(source_id: SourceId, ast: &Root, diag: &mut Diagnostics) {
    let mut visitor = EmptyConstructsValidator { diag, source_id };
    visitor.visit(ast);
}

struct EmptyConstructsValidator<'a> {
    diag: &'a mut Diagnostics,
    source_id: SourceId,
}

impl Visitor for EmptyConstructsValidator<'_> {
    fn visit_named_node(&mut self, node: &NamedNode) {
        // Check for truly empty tree: no child nodes at all in CST (only tokens like parens)
        // This excludes invalid content like predicates which create Error nodes
        if node.as_cst().children().next().is_none() && node.node_type().is_none() {
            self.diag
                .report(self.source_id, DiagnosticKind::EmptyTree, node.text_range())
                .emit();
        }
        walk_named_node(self, node);
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        if seq.children().next().is_none() {
            self.diag
                .report(
                    self.source_id,
                    DiagnosticKind::EmptySequence,
                    seq.text_range(),
                )
                .emit();
        }
        walk_seq_expr(self, seq);
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        if alt.branches().next().is_none() {
            self.diag
                .report(
                    self.source_id,
                    DiagnosticKind::EmptyAlternation,
                    alt.text_range(),
                )
                .emit();
        }
        walk_alt_expr(self, alt);
    }
}
