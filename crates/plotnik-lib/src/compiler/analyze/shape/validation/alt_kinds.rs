//! Mixed-labeling alternation diagnostic.

use super::ValidationInput;
use crate::compiler::analyze::shape::invariants::ensure_both_labeling_kinds;
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{Alternative, Labeling, Pattern};
use crate::compiler::parse::cst::SyntaxKind;

pub fn validate_alt_kinds(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;

    for node in ast.syntax().descendants() {
        if node.kind() != SyntaxKind::Alternation {
            continue;
        }
        let Some(Pattern::Alternation(alternation)) = Pattern::cast(node) else {
            continue;
        };
        if alternation.labeling() != Labeling::Mixed {
            continue;
        }

        let alternatives: Vec<Alternative> = alternation.alternatives().collect();
        let first_labeled = alternatives.iter().find(|a| a.label().is_some());
        let first_unlabeled = alternatives.iter().find(|a| a.label().is_none());
        let (labeled, unlabeled) = ensure_both_labeling_kinds(first_labeled, first_unlabeled);

        let labeled_range = labeled
            .label()
            .expect("labeled alternative found via filter must have label")
            .text_range();

        diag.report(
            DiagnosticKind::MixedAltBranches,
            Span::new(source_id, unlabeled.text_range()),
        )
        .related_to(
            Span::new(source_id, labeled_range),
            "labeled alternative here",
        )
        .emit();
    }
}
