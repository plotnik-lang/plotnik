//! Semantic validation for the typed AST.
//!
//! Checks constraints that are easier to express after parsing:
//! - Mixed tagged/untagged alternations

use rowan::TextRange;

use crate::ast::{Alt, AltKind, Branch, Expr, Root};
use crate::parser::{ErrorStage, RelatedInfo, SyntaxError};

pub fn validate(root: &Root) -> Vec<SyntaxError> {
    let mut errors = Vec::new();

    for def in root.defs() {
        if let Some(body) = def.body() {
            validate_expr(&body, &mut errors);
        }
    }

    for expr in root.exprs() {
        validate_expr(&expr, &mut errors);
    }

    errors
}

fn validate_expr(expr: &Expr, errors: &mut Vec<SyntaxError>) {
    match expr {
        Expr::Alt(alt) => {
            check_mixed_alternation(alt, errors);
            for branch in alt.branches() {
                if let Some(body) = branch.body() {
                    validate_expr(&body, errors);
                }
            }
            for child in alt.exprs() {
                validate_expr(&child, errors);
            }
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
        | Expr::Lit(_)
        | Expr::Str(_)
        | Expr::Wildcard(_)
        | Expr::Anchor(_)
        | Expr::NegatedField(_) => {}
    }
}

fn check_mixed_alternation(alt: &Alt, errors: &mut Vec<SyntaxError>) {
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
        return;
    };

    let tagged_range = tagged_branch
        .label()
        .map(|t| t.text_range())
        .unwrap_or_else(|| branch_range(tagged_branch));

    let untagged_range = branch_range(untagged_branch);

    let error = SyntaxError::with_related(
        untagged_range,
        "mixed tagged and untagged branches in alternation",
        RelatedInfo::new(tagged_range, "tagged branch here"),
    )
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
        Root
          Def
            Alt
              Branch A:
                Tree a
              Branch
                Tree b
        ---
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
        Root
          Def
            Alt
              Branch
                Tree a
              Branch B:
                Tree b
        ---
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
        Root
          Def
            Tree call
              Alt
                Branch A:
                  Tree a
                Branch
                  Tree b
        ---
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
        Root
          Def
            Tree foo
              Alt
                Branch A:
                  Tree a
                Branch
                  Tree b
              Alt
                Branch C:
                  Tree c
                Branch
                  Tree d
        ---
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
        insta::assert_snapshot!(query.snapshot_ast(), @r"
        Root
          Def
            Alt
              Branch A:
                Tree a
        ");
    }
}
