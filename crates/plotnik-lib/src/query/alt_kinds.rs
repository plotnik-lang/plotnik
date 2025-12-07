//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use super::Query;
use super::invariants::{
    assert_alt_no_bare_exprs, assert_root_no_bare_exprs, ensure_both_branch_kinds,
};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AltExpr, AltKind, Branch, Expr};

impl Query<'_> {
    pub(super) fn validate_alt_kinds(&mut self) {
        let defs: Vec<_> = self.ast.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            self.validate_alt_expr(&body);
        }

        assert_root_no_bare_exprs(&self.ast);
    }

    fn validate_alt_expr(&mut self, expr: &Expr) {
        if let Expr::AltExpr(alt) = expr {
            self.check_mixed_alternation(alt);
            assert_alt_no_bare_exprs(alt);
        }

        for child in expr.children() {
            self.validate_alt_expr(&child);
        }
    }

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

        self.alt_kind_diagnostics
            .report(DiagnosticKind::MixedAltBranches, untagged_range)
            .related_to("tagged branch here", tagged_range)
            .emit();
    }
}

fn branch_range(branch: &Branch) -> TextRange {
    branch.text_range()
}
