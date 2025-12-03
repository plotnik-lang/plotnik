//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use super::invariants::{
    assert_alt_no_bare_exprs, assert_root_no_bare_exprs, ensure_both_branch_kinds,
};
use crate::ast::{Alt, AltKind, Branch, Expr, Root};
use crate::ast::{Diagnostic, ErrorStage, RelatedInfo};

pub fn validate(root: &Root) -> Vec<Diagnostic> {
    let mut errors = Vec::new();

    for def in root.defs() {
        if let Some(body) = def.body() {
            validate_expr(&body, &mut errors);
        }
    }

    assert_root_no_bare_exprs(root);

    errors
}

fn validate_expr(expr: &Expr, errors: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Alt(alt) => {
            check_mixed_alternation(alt, errors);
            for branch in alt.branches() {
                if let Some(body) = branch.body() {
                    validate_expr(&body, errors);
                }
            }
            assert_alt_no_bare_exprs(alt);
        }
        Expr::Tree(tree) => {
            for child in tree.children() {
                validate_expr(&child, errors);
            }
        }
        Expr::Seq(seq) => {
            for child in seq.children() {
                validate_expr(&child, errors);
            }
        }
        Expr::Capture(cap) => {
            if let Some(inner) = cap.inner() {
                validate_expr(&inner, errors);
            }
        }
        Expr::Quantifier(q) => {
            if let Some(inner) = q.inner() {
                validate_expr(&inner, errors);
            }
        }
        Expr::Field(f) => {
            if let Some(value) = f.value() {
                validate_expr(&value, errors);
            }
        }
        Expr::Ref(_)
        | Expr::Str(_)
        | Expr::Wildcard(_)
        | Expr::Anchor(_)
        | Expr::NegatedField(_) => {}
    }
}

fn check_mixed_alternation(alt: &Alt, errors: &mut Vec<Diagnostic>) {
    if alt.kind() != AltKind::Mixed {
        return;
    }

    let branches: Vec<Branch> = alt.branches().collect();

    let mut first_tagged: Option<&Branch> = None;
    let mut first_untagged: Option<&Branch> = None;

    for branch in &branches {
        if branch.label().is_some() {
            if first_tagged.is_none() {
                first_tagged = Some(branch);
            }
        } else if first_untagged.is_none() {
            first_untagged = Some(branch);
        }

        if first_tagged.is_some() && first_untagged.is_some() {
            break;
        }
    }

    let (tagged_branch, untagged_branch) = ensure_both_branch_kinds(first_tagged, first_untagged);

    let tagged_range = tagged_branch
        .label()
        .map(|t| t.text_range())
        .unwrap_or_else(|| branch_range(tagged_branch));

    let untagged_range = branch_range(untagged_branch);

    let error = Diagnostic::error(
        untagged_range,
        "mixed tagged and untagged branches in alternation",
    )
    .with_related(RelatedInfo::new(tagged_range, "tagged branch here"))
    .with_stage(ErrorStage::Validate);

    errors.push(error);
}

fn branch_range(branch: &Branch) -> TextRange {
    branch.syntax().text_range()
}
