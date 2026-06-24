//! Mixed enum/union alternation diagnostic.
//!
//! Runs over the raw syntax tree rather than the typed `Pattern` split: union
//! and enum are distinct AST nodes, but "do some branches have labels and others
//! not" is a purely syntactic question. Classifying here keeps the typed split
//! mixed-blind while preserving a precise diagnostic for `[A: (x) (y)]`.

use super::ValidationInput;
use crate::compiler::analyze::shape::invariants::ensure_both_branch_kinds;
use crate::compiler::core::SyntaxKind;
use crate::compiler::parse::ast::{AltKind, Branch, classify_alt};
use crate::compiler::diagnostics::diagnostics::DiagnosticKind;

pub fn validate_alt_kinds(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;

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

        diag.report(
            source_id,
            DiagnosticKind::MixedAltBranches,
            union_branch.text_range(),
        )
        .related_to(source_id, enum_range, "enum branch here")
        .emit();
    }
}
