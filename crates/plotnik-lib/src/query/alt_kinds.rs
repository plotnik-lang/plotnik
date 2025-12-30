//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use super::invariants::ensure_both_branch_kinds;
use super::visitor::{Visitor, walk, walk_alt_expr};
use crate::SourceId;
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{AltExpr, AltKind, Branch, Root};

pub fn validate_alt_kinds(source_id: SourceId, ast: &Root, diag: &mut Diagnostics) {
    let mut visitor = AltKindsValidator { diag, source_id };
    visitor.visit(ast);
}

struct AltKindsValidator<'a> {
    diag: &'a mut Diagnostics,
    source_id: SourceId,
}

impl Visitor for AltKindsValidator<'_> {
    fn visit(&mut self, root: &Root) {
        assert!(
            root.exprs().next().is_none(),
            "alt_kind: unexpected bare Expr in Root (parser should wrap in Def)"
        );
        walk(self, root);
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        self.check_mixed_alternation(alt);
        assert!(
            alt.exprs().next().is_none(),
            "alt_kind: unexpected bare Expr in Alt (parser should wrap in Branch)"
        );
        walk_alt_expr(self, alt);
    }
}

impl AltKindsValidator<'_> {
    fn check_mixed_alternation(&mut self, alt: &AltExpr) {
        if alt.kind() != AltKind::Mixed {
            return;
        }

        let branches: Vec<Branch> = alt.branches().collect();
        let first_tagged = branches.iter().find(|b| b.label().is_some());
        let first_untagged = branches.iter().find(|b| b.label().is_none());

        let (tagged_branch, untagged_branch) =
            ensure_both_branch_kinds(first_tagged, first_untagged);

        let tagged_range = tagged_branch
            .label()
            .expect("tagged branch found via filter must have label")
            .text_range();

        self.diag
            .report(
                self.source_id,
                DiagnosticKind::MixedAltBranches,
                untagged_branch.text_range(),
            )
            .related_to(self.source_id, tagged_range, "tagged branch here")
            .emit();
    }
}
