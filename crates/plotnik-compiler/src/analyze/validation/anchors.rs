//! Semantic validation for anchor placement.
//!
//! Anchors require context to be meaningful:
//! - **Boundary anchors** (at start/end of sequence) need parent named node context
//! - **Interior anchors** (between items) are always valid
//!
//! This validation ensures anchors are placed where they can be meaningfully compiled.

use super::ValidateInput;
use crate::analyze::Reporter;
use crate::analyze::visitor::{Visitor, walk_named_node, walk_seq_expr};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{NamedNode, SeqExpr, SeqItem};

pub fn validate_anchors(input: ValidateInput) {
    let ValidateInput {
        source_id,
        ast,
        diag,
        ..
    } = input;
    let mut visitor = AnchorValidator {
        reporter: Reporter::new(source_id, diag),
        in_named_node: false,
    };
    visitor.visit(ast);
}

struct AnchorValidator<'a> {
    reporter: Reporter<'a>,
    in_named_node: bool,
}

impl Visitor for AnchorValidator<'_> {
    fn visit_named_node(&mut self, node: &NamedNode) {
        let prev = self.in_named_node;
        self.in_named_node = true;

        self.check_items(node.items());

        // Named node provides first/last/adjacent context, so any anchor inside is valid.
        walk_named_node(self, node);

        self.in_named_node = prev;
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        self.check_items(seq.items());

        walk_seq_expr(self, seq);
    }
}

impl AnchorValidator<'_> {
    fn check_items(&mut self, items: impl Iterator<Item = SeqItem>) {
        let items: Vec<_> = items.collect();
        let len = items.len();

        for (i, item) in items.iter().enumerate() {
            if let SeqItem::Anchor(anchor) = item {
                let is_boundary = i == 0 || i == len - 1;

                if is_boundary && !self.in_named_node {
                    self.reporter
                        .report(DiagnosticKind::AnchorWithoutContext, anchor.text_range())
                        .emit();
                }
            }
        }
    }
}
