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
        match expr {
            Expr::AltExpr(alt) => {
                self.check_mixed_alternation(alt);
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.validate_alt_expr(&body);
                }
                assert_alt_no_bare_exprs(alt);
            }
            Expr::NamedNode(node) => {
                for child in node.children() {
                    self.validate_alt_expr(&child);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.validate_alt_expr(&child);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.validate_alt_expr(&inner);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.validate_alt_expr(&inner);
            }
            Expr::FieldExpr(f) => {
                let Some(value) = f.value() else { return };
                self.validate_alt_expr(&value);
            }
            Expr::Ref(_) | Expr::AnonymousNode(_) => {}
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
            .message("mixed tagged and untagged branches in alternation")
            .related_to("tagged branch here", tagged_range)
            .emit();
    }
}

fn branch_range(branch: &Branch) -> TextRange {
    branch.text_range()
}
