//! Semantic validation for anchor placement.
//!
//! Anchors require context to be meaningful:
//! - **Boundary anchors** (at start/end of sequence) need parent named node context
//! - **Interior anchors** (between items) are always valid
//!
//! This validation ensures anchors are placed where they can be meaningfully compiled.

use super::ValidationInput;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk_node_pattern, walk_seq_pattern};
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{NodePattern, SeqItem, SeqPattern};

pub fn validate_anchors(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut visitor = AnchorValidator {
        diag,
        in_named_node: false,
    };
    visitor.visit(&Located::new(source_id, ast.clone()));
}

struct AnchorValidator<'d> {
    diag: &'d mut Diagnostics,
    in_named_node: bool,
}

impl Visitor for AnchorValidator<'_> {
    fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
        let prev = self.in_named_node;
        self.in_named_node = true;

        self.check_items(node.source(), node.node().items());

        // Named node provides first/last/adjacent context, so any anchor inside is valid.
        walk_node_pattern(self, node);

        self.in_named_node = prev;
    }

    fn visit_seq_pattern(&mut self, seq: &Located<SeqPattern>) {
        self.check_items(seq.source(), seq.node().items());

        walk_seq_pattern(self, seq);
    }
}

impl AnchorValidator<'_> {
    fn check_items(&mut self, source: SourceId, items: impl Iterator<Item = SeqItem>) {
        let items: Vec<_> = items.collect();
        let len = items.len();

        for (i, item) in items.iter().enumerate() {
            if let SeqItem::Anchor(anchor) = item {
                let is_boundary = i == 0 || i == len - 1;

                if is_boundary && !self.in_named_node {
                    self.diag
                        .report(
                            DiagnosticKind::AnchorWithoutContext,
                            Span::new(source, anchor.text_range()),
                        )
                        .emit();
                }
            }
        }
    }
}
