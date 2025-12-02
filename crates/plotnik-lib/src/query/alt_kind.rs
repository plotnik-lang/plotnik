//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use crate::ast::{Alt, AltKind, Branch, Expr, Root};
use crate::ast::{Diagnostic, ErrorStage, RelatedInfo};

pub fn validate(root: &Root) -> Vec<Diagnostic> {
    let mut errors = Vec::new();

    for def in root.defs() {
        if let Some(body) = def.body() {
            validate_expr(&body, &mut errors);
        }
    }

    // Parser wraps all top-level exprs in Def nodes, so this should be empty
    assert!(
        root.exprs().next().is_none(),
        "alt_kind: unexpected bare Expr in Root (parser should wrap in Def)"
    );

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
            // Parser wraps all alt children in Branch nodes
            assert!(
                alt.exprs().next().is_none(),
                "alt_kind: unexpected bare Expr in Alt (parser should wrap in Branch)"
            );
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

    let (Some(tagged_branch), Some(untagged_branch)) = (first_tagged, first_untagged) else {
        panic!(
            "alt_kind: Mixed alternation without both tagged and untagged branches (should be caught by AltKind::compute_kind)"
        );
    };

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

#[cfg(test)]
mod tests {
    use crate::Query;

    #[test]
    fn tagged_alternation_valid() {
        let query = Query::new("[A: (a) B: (b)]");
        assert!(query.is_valid());
        insta::assert_snapshot!(query.dump_ast(), @r"
        Root
          Def
            Alt
              Branch A:
                Tree a
              Branch B:
                Tree b
        ");
    }

    #[test]
    fn untagged_alternation_valid() {
        let query = Query::new("[(a) (b)]");
        assert!(query.is_valid());
        insta::assert_snapshot!(query.dump_ast(), @r"
        Root
          Def
            Alt
              Branch
                Tree a
              Branch
                Tree b
        ");
    }

    #[test]
    fn mixed_alternation_tagged_first() {
        let query = Query::new("[A: (a) (b)]");
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.dump_errors(), @r"
        error: mixed tagged and untagged branches in alternation
          |
        1 | [A: (a) (b)]
          |  -      ^^^ mixed tagged and untagged branches in alternation
          |  |
          |  tagged branch here
        ");
    }

    #[test]
    fn mixed_alternation_untagged_first() {
        let query = Query::new(
            r#"
        [
          (a)
          B: (b)
        ]
        "#,
        );
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.dump_errors(), @r"
        error: mixed tagged and untagged branches in alternation
          |
        3 |           (a)
          |           ^^^ mixed tagged and untagged branches in alternation
        4 |           B: (b)
          |           - tagged branch here
        ");
    }

    #[test]
    fn nested_mixed_alternation() {
        let query = Query::new("(call [A: (a) (b)])");
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.dump_errors(), @r"
        error: mixed tagged and untagged branches in alternation
          |
        1 | (call [A: (a) (b)])
          |        -      ^^^ mixed tagged and untagged branches in alternation
          |        |
          |        tagged branch here
        ");
    }

    #[test]
    fn multiple_mixed_alternations() {
        let query = Query::new("(foo [A: (a) (b)] [C: (c) (d)])");
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.dump_errors(), @r"
        error: mixed tagged and untagged branches in alternation
          |
        1 | (foo [A: (a) (b)] [C: (c) (d)])
          |       -      ^^^ mixed tagged and untagged branches in alternation
          |       |
          |       tagged branch here
        error: mixed tagged and untagged branches in alternation
          |
        1 | (foo [A: (a) (b)] [C: (c) (d)])
          |                    -      ^^^ mixed tagged and untagged branches in alternation
          |                    |
          |                    tagged branch here
        ");
    }

    #[test]
    fn single_branch_no_error() {
        let query = Query::new("[A: (a)]");
        assert!(query.is_valid());
        insta::assert_snapshot!(query.dump_ast(), @r"
        Root
          Def
            Alt
              Branch A:
                Tree a
        ");
    }
}
