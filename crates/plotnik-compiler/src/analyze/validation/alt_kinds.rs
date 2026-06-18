//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed enum/union alternations

use super::ValidationInput;
use crate::analyze::Reporter;
use crate::analyze::invariants::ensure_both_branch_kinds;
use crate::analyze::visitor::{Visitor, walk, walk_alt_pattern};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AltPattern, AltKind, Branch, Root};

pub fn validate_alt_kinds(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut visitor = AltKindsValidator {
        reporter: Reporter::new(source_id, diag),
    };
    visitor.visit(ast);
}

struct AltKindsValidator<'a> {
    reporter: Reporter<'a>,
}

impl Visitor for AltKindsValidator<'_> {
    fn visit(&mut self, root: &Root) {
        assert!(
            root.patterns().next().is_none(),
            "alt_kind: unexpected bare Pattern in Root (parser should wrap in Def)"
        );
        walk(self, root);
    }

    fn visit_alt_pattern(&mut self, alt: &AltPattern) {
        self.check_mixed_alternation(alt);
        assert!(
            alt.patterns().next().is_none(),
            "alt_kind: unexpected bare Pattern in Alt (parser should wrap in Branch)"
        );
        walk_alt_pattern(self, alt);
    }
}

impl AltKindsValidator<'_> {
    fn check_mixed_alternation(&mut self, alt: &AltPattern) {
        if alt.kind() != AltKind::Mixed {
            return;
        }

        let branches: Vec<Branch> = alt.branches().collect();
        let first_enum = branches.iter().find(|b| b.label().is_some());
        let first_union = branches.iter().find(|b| b.label().is_none());

        let (enum_branch, unenum_branch) =
            ensure_both_branch_kinds(first_enum, first_union);

        let enum_range = enum_branch
            .label()
            .expect("enum branch found via filter must have label")
            .text_range();

        let source = self.reporter.source();
        self.reporter
            .report(
                DiagnosticKind::MixedAltBranches,
                unenum_branch.text_range(),
            )
            .related_to(source, enum_range, "enum branch here")
            .emit();
    }
}
