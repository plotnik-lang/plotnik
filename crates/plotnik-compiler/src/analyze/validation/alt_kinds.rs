//! Mixed enum/union alternation diagnostic.
//!
//! Runs over the raw syntax tree rather than the typed `Pattern` split: union
//! and enum are distinct AST nodes, but "are some branches labeled and others
//! not" is a purely syntactic question. Classifying here keeps the typed split
//! mixed-blind while preserving a precise diagnostic for `[A: (x) (y)]`.

use super::ValidationInput;
use crate::analyze::Reporter;
use crate::analyze::invariants::ensure_both_branch_kinds;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AltKind, Branch, SyntaxKind, classify_alt};

pub fn validate_alt_kinds(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut reporter = Reporter::new(source_id, diag);

    for node in ast.syntax().descendants() {
        if node.kind() != SyntaxKind::Alt || classify_alt(&node) != AltKind::Mixed {
            continue;
        }

        let branches: Vec<Branch> = node.children().filter_map(Branch::cast).collect();
        let first_enum = branches.iter().find(|b| b.label().is_some());
        let first_union = branches.iter().find(|b| b.label().is_none());
        let (enum_branch, union_branch) = ensure_both_branch_kinds(first_enum, first_union);

        let enum_range = enum_branch
            .label()
            .expect("enum branch found via filter must have label")
            .text_range();

        let source = reporter.source();
        reporter
            .report(DiagnosticKind::MixedAltBranches, union_branch.text_range())
            .related_to(source, enum_range, "enum branch here")
            .emit();
    }
}
