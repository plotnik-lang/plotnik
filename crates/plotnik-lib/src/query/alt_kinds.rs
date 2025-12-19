//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use super::invariants::ensure_both_branch_kinds;
use super::visitor::{Visitor, walk, walk_alt_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{AltExpr, AltKind, Branch, Root};

pub fn validate_alt_kinds(ast: &Root, diag: &mut Diagnostics) {
    let mut visitor = AltKindsValidator { diag };
    visitor.visit(ast);
}

struct AltKindsValidator<'a> {
    diag: &'a mut Diagnostics,
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
            .map(|t| t.text_range())
            .unwrap_or_else(|| branch_range(tagged_branch));

        let untagged_range = branch_range(untagged_branch);

        self.diag
            .report(DiagnosticKind::MixedAltBranches, untagged_range)
            .related_to("tagged branch here", tagged_range)
            .emit();
    }
}

fn branch_range(branch: &Branch) -> TextRange {
    branch.text_range()
}
