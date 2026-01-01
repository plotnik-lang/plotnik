//! Semantic validation for anchor placement.
//!
//! Anchors require context to be meaningful:
//! - **Boundary anchors** (at start/end of sequence) need parent named node context
//! - **Interior anchors** (between items) are always valid
//!
//! This validation ensures anchors are placed where they can be meaningfully compiled.

use crate::SourceId;
use crate::analyze::visitor::{Visitor, walk_named_node, walk_seq_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::ast::{NamedNode, Root, SeqExpr, SeqItem};

pub fn validate_anchors(source_id: SourceId, ast: &Root, diag: &mut Diagnostics) {
    let mut visitor = AnchorValidator {
        diag,
        source_id,
        in_named_node: false,
    };
    visitor.visit(ast);
}

struct AnchorValidator<'a> {
    diag: &'a mut Diagnostics,
    source_id: SourceId,
    in_named_node: bool,
}

impl Visitor for AnchorValidator<'_> {
    fn visit_named_node(&mut self, node: &NamedNode) {
        let prev = self.in_named_node;
        self.in_named_node = true;

        // Check for anchors in the named node's items
        self.check_items(node.items());

        // Anchors inside named node children are always valid
        // (the node provides first/last/adjacent context)
        walk_named_node(self, node);

        self.in_named_node = prev;
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        // Check for boundary anchors without context
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
                    self.diag
                        .report(
                            self.source_id,
                            DiagnosticKind::AnchorWithoutContext,
                            anchor.text_range(),
                        )
                        .emit();
                }
            }
        }
    }
}
