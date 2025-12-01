//! Name resolution: builds symbol table and checks references.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use crate::ql::ast::{Expr, Ref, Root};
use crate::ql::parser::SyntaxError;
use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

#[derive(Debug, Clone)]
pub struct SymbolTable {
    defs: IndexMap<String, DefInfo>,
}

#[derive(Debug, Clone)]
pub struct DefInfo {
    pub name: String,
    pub range: TextRange,
    pub refs: IndexSet<String>,
}

#[derive(Debug)]
pub struct ResolveResult {
    pub symbols: SymbolTable,
    pub errors: Vec<SyntaxError>,
}

impl SymbolTable {
    pub fn get(&self, name: &str) -> Option<&DefInfo> {
        self.defs.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.defs.keys().map(|s| s.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &DefInfo> {
        self.defs.values()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

pub fn resolve(root: &Root) -> ResolveResult {
    let mut defs = IndexMap::new();
    let mut errors = Vec::new();

    // Pass 1: collect definitions
    for def in root.defs() {
        if let Some(name_token) = def.name() {
            let name = name_token.text().to_string();
            let range = name_token.text_range();

            if defs.contains_key(&name) {
                errors.push(SyntaxError::new(
                    range,
                    format!("duplicate definition: `{}`", name),
                ));
            } else {
                let mut refs = IndexSet::new();
                if let Some(body) = def.body() {
                    collect_refs(&body, &mut refs);
                }
                defs.insert(name.clone(), DefInfo { name, range, refs });
            }
        }
    }

    let symbols = SymbolTable { defs };

    // Pass 2: check references
    for def in root.defs() {
        if let Some(body) = def.body() {
            collect_reference_errors(&body, &symbols, &mut errors);
        }
    }

    // Also check top-level expressions (entry point)
    for expr in root.exprs() {
        collect_reference_errors(&expr, &symbols, &mut errors);
    }

    ResolveResult { symbols, errors }
}

fn collect_refs(expr: &Expr, refs: &mut IndexSet<String>) {
    match expr {
        Expr::Ref(r) => {
            if let Some(name_token) = r.name() {
                refs.insert(name_token.text().to_string());
            }
        }
        Expr::Tree(tree) => {
            for child in tree.children() {
                collect_refs(&child, refs);
            }
        }
        Expr::Alt(alt) => {
            for branch in alt.branches() {
                if let Some(body) = branch.body() {
                    collect_refs(&body, refs);
                }
            }
            for expr in alt.exprs() {
                collect_refs(&expr, refs);
            }
        }
        Expr::Seq(seq) => {
            for child in seq.children() {
                collect_refs(&child, refs);
            }
        }
        Expr::Capture(cap) => {
            if let Some(inner) = cap.inner() {
                collect_refs(&inner, refs);
            }
        }
        Expr::Quantifier(q) => {
            if let Some(inner) = q.inner() {
                collect_refs(&inner, refs);
            }
        }
        Expr::Field(f) => {
            if let Some(value) = f.value() {
                collect_refs(&value, refs);
            }
        }
        Expr::Lit(_) | Expr::Wildcard(_) | Expr::Anchor(_) | Expr::NegatedField(_) => {}
    }
}

fn collect_reference_errors(expr: &Expr, symbols: &SymbolTable, errors: &mut Vec<SyntaxError>) {
    match expr {
        Expr::Ref(r) => {
            check_ref_reference(r, symbols, errors);
        }
        Expr::Tree(tree) => {
            for child in tree.children() {
                collect_reference_errors(&child, symbols, errors);
            }
        }
        Expr::Alt(alt) => {
            for branch in alt.branches() {
                if let Some(body) = branch.body() {
                    collect_reference_errors(&body, symbols, errors);
                }
            }
            for expr in alt.exprs() {
                collect_reference_errors(&expr, symbols, errors);
            }
        }
        Expr::Seq(seq) => {
            for child in seq.children() {
                collect_reference_errors(&child, symbols, errors);
            }
        }
        Expr::Capture(cap) => {
            if let Some(inner) = cap.inner() {
                collect_reference_errors(&inner, symbols, errors);
            }
        }
        Expr::Quantifier(q) => {
            if let Some(inner) = q.inner() {
                collect_reference_errors(&inner, symbols, errors);
            }
        }
        Expr::Field(f) => {
            if let Some(value) = f.value() {
                collect_reference_errors(&value, symbols, errors);
            }
        }
        Expr::Lit(_) | Expr::Wildcard(_) | Expr::Anchor(_) | Expr::NegatedField(_) => {}
    }
}

fn check_ref_reference(r: &Ref, symbols: &SymbolTable, errors: &mut Vec<SyntaxError>) {
    if let Some(name_token) = r.name() {
        let name = name_token.text();
        if symbols.get(name).is_none() {
            errors.push(SyntaxError::new(
                name_token.text_range(),
                format!("undefined reference: `{}`", name),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Query;
    use indoc::indoc;

    #[test]
    fn single_definition() {
        let input = "Expr = (expression)";
        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @"Expr");
    }

    #[test]
    fn multiple_definitions() {
        let input = indoc! {r#"
        Expr = (expression)
        Stmt = (statement)
        Decl = (declaration)
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Decl
        Expr
        Stmt
        ");
    }

    #[test]
    fn valid_reference() {
        let input = indoc! {r#"
        Expr = (expression)
        Call = (call_expression function: (Expr))
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Call -> Expr
        Expr
        ");
    }

    #[test]
    fn undefined_reference() {
        let input = "Call = (call_expression function: (Undefined))";

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Call -> Undefined
        ---
        error: undefined reference: `Undefined`
          |
        1 | Call = (call_expression function: (Undefined))
          |                                    ^^^^^^^^^ undefined reference: `Undefined`
        ");
    }

    #[test]
    fn self_reference() {
        let input = "Expr = [(identifier) (call (Expr))]";

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @"Expr -> Expr");
    }

    #[test]
    fn mutual_recursion() {
        let input = indoc! {r#"
        A = (foo (B))
        B = (bar (A))
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        A -> B
        B -> A
        ---
        error: recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
          |
        1 | A = (foo (B))
          |           - `A` references `B` (completing cycle)
        2 | B = (bar (A))
          |           ^
          |           |
          |           recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
          |           `B` references `A`
        ");
    }

    #[test]
    fn duplicate_definition() {
        let input = indoc! {r#"
        Expr = (expression)
        Expr = (other)
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Expr
        ---
        error: duplicate definition: `Expr`
          |
        2 | Expr = (other)
          | ^^^^ duplicate definition: `Expr`
        ");
    }

    #[test]
    fn reference_in_alternation() {
        let input = indoc! {r#"
        Expr = (expression)
        Value = [(Expr) (literal)]
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Expr
        Value -> Expr
        ");
    }

    #[test]
    fn reference_in_sequence() {
        let input = indoc! {r#"
        Expr = (expression)
        Pair = {(Expr) (Expr)}
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Expr
        Pair -> Expr
        ");
    }

    #[test]
    fn reference_in_quantifier() {
        let input = indoc! {r#"
        Expr = (expression)
        List = (Expr)*
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Expr
        List -> Expr
        ");
    }

    #[test]
    fn reference_in_capture() {
        let input = indoc! {r#"
        Expr = (expression)
        Named = (Expr) @e
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        Expr
        Named -> Expr
        ");
    }

    #[test]
    fn entry_point_reference() {
        let input = indoc! {r#"
        Expr = (expression)
        (call function: (Expr))
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @"Expr");
    }

    #[test]
    fn entry_point_undefined_reference() {
        let input = "(call function: (Unknown))";

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        ---
        error: undefined reference: `Unknown`
          |
        1 | (call function: (Unknown))
          |                  ^^^^^^^ undefined reference: `Unknown`
        ");
    }

    #[test]
    fn no_definitions() {
        let input = "(identifier)";
        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @"");
    }

    #[test]
    fn nested_references() {
        let input = indoc! {r#"
        A = (a)
        B = (b (A))
        C = (c (B))
        D = (d (C) (A))
        "#};

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        A
        B -> A
        C -> B
        D -> A, C
        ");
    }

    #[test]
    fn multiple_undefined() {
        let input = "(foo (X) (Y) (Z))";

        let query = Query::new(input);
        insta::assert_snapshot!(query.snapshot_refs(), @r"
        ---
        error: undefined reference: `X`
          |
        1 | (foo (X) (Y) (Z))
          |       ^ undefined reference: `X`
        error: undefined reference: `Y`
          |
        1 | (foo (X) (Y) (Z))
          |           ^ undefined reference: `Y`
        error: undefined reference: `Z`
          |
        1 | (foo (X) (Y) (Z))
          |               ^ undefined reference: `Z`
        ");
    }
}
