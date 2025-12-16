//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use super::Query;
use super::invariants::ensure_both_branch_kinds;
use super::visitor::{Visitor, walk_alt_expr, walk_root};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{AltExpr, AltKind, Branch, Root};

impl Query<'_> {
    pub(super) fn validate_alt_kinds(&mut self) {
        let mut visitor = AltKindsValidator {
            diagnostics: &mut self.alt_kind_diagnostics,
        };
        visitor.visit_root(&self.ast);
    }
}

struct AltKindsValidator<'a> {
    diagnostics: &'a mut Diagnostics,
}

impl Visitor for AltKindsValidator<'_> {
    fn visit_root(&mut self, root: &Root) {
        assert!(
            root.exprs().next().is_none(),
            "alt_kind: unexpected bare Expr in Root (parser should wrap in Def)"
        );
        walk_root(self, root);
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
            .map(|t| t.text_range())
            .unwrap_or_else(|| branch_range(tagged_branch));

        let untagged_range = branch_range(untagged_branch);

        self.diagnostics
            .report(DiagnosticKind::MixedAltBranches, untagged_range)
            .related_to("tagged branch here", tagged_range)
            .emit();
    }
}

fn branch_range(branch: &Branch) -> TextRange {
    branch.text_range()
}
