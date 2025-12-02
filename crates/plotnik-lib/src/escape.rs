//! Escape path analysis for recursive definitions.
//!
//! Detects patterns that can never match because they require
//! infinitely nested structures (recursion with no escape path).

use crate::ast::{Def, Expr, Root};
use crate::parser::{ErrorStage, RelatedInfo, SyntaxError};
use crate::resolve::SymbolTable;
use crate::syntax_kind::SyntaxKind;
use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

pub fn check_escape(root: &Root, symbols: &SymbolTable) -> Vec<SyntaxError> {
    let sccs = find_sccs(symbols);
    let mut errors = Vec::new();

    for scc in sccs {
        if scc.len() == 1 {
            let name = &scc[0];
            if let Some(def_info) = symbols.get(name) {
                if def_info.refs.contains(name) {
                    if let Some(def) = find_def_by_name(root, name) {
                        if let Some(body) = def.body() {
                            let scc_set: IndexSet<&str> = std::iter::once(name.as_str()).collect();
                            if !expr_has_escape(&body, &scc_set) {
                                let chain = build_self_ref_chain(root, name);
                                errors.push(make_error(name, &scc, chain));
                            }
                        }
                    }
                }
            }
        } else {
            let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();
            let mut all_have_escape = true;

            for name in &scc {
                if let Some(def) = find_def_by_name(root, name) {
                    if let Some(body) = def.body() {
                        if !expr_has_escape(&body, &scc_set) {
                            all_have_escape = false;
                            break;
                        }
                    }
                }
            }

            if !all_have_escape {
                let chain = build_cycle_chain(root, symbols, &scc);
                errors.push(make_error(&scc[0], &scc, chain));
            }
        }
    }

    errors
}

fn expr_has_escape(expr: &Expr, scc: &IndexSet<&str>) -> bool {
    match expr {
        Expr::Ref(r) => {
            // A Ref is always a reference to a user-defined expression
            // If it's in the SCC, it doesn't provide an escape path
            if let Some(name_token) = r.name() {
                !scc.contains(name_token.text())
            } else {
                true
            }
        }
        Expr::Tree(tree) => {
            let children: Vec<_> = tree.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc))
        }

        Expr::Alt(alt) => {
            alt.branches().any(|b| {
                b.body()
                    .map(|body| expr_has_escape(&body, scc))
                    .unwrap_or(true)
            }) || alt.exprs().any(|e| expr_has_escape(&e, scc))
        }

        Expr::Seq(seq) => seq.children().all(|c| expr_has_escape(&c, scc)),

        Expr::Quantifier(q) => match q.operator().map(|op| op.kind()) {
            Some(
                SyntaxKind::Question
                | SyntaxKind::Star
                | SyntaxKind::QuestionQuestion
                | SyntaxKind::StarQuestion,
            ) => true,
            Some(SyntaxKind::Plus | SyntaxKind::PlusQuestion) => q
                .inner()
                .map(|inner| expr_has_escape(&inner, scc))
                .unwrap_or(true),
            _ => true,
        },

        Expr::Capture(cap) => cap
            .inner()
            .map(|inner| expr_has_escape(&inner, scc))
            .unwrap_or(true),

        Expr::Field(f) => f.value().map(|v| expr_has_escape(&v, scc)).unwrap_or(true),

        Expr::Lit(_)
        | Expr::Str(_)
        | Expr::Wildcard(_)
        | Expr::Anchor(_)
        | Expr::NegatedField(_) => true,
    }
}

fn find_sccs(symbols: &SymbolTable) -> Vec<Vec<String>> {
    struct State<'a> {
        symbols: &'a SymbolTable,
        index: usize,
        stack: Vec<String>,
        on_stack: IndexSet<String>,
        indices: IndexMap<String, usize>,
        lowlinks: IndexMap<String, usize>,
        sccs: Vec<Vec<String>>,
    }

    fn strongconnect(name: &str, state: &mut State<'_>) {
        state.indices.insert(name.to_string(), state.index);
        state.lowlinks.insert(name.to_string(), state.index);
        state.index += 1;
        state.stack.push(name.to_string());
        state.on_stack.insert(name.to_string());

        if let Some(def_info) = state.symbols.get(name) {
            for ref_name in &def_info.refs {
                if state.symbols.get(ref_name).is_none() {
                    continue;
                }
                if !state.indices.contains_key(ref_name) {
                    strongconnect(ref_name, state);
                    let ref_lowlink = state.lowlinks[ref_name];
                    let my_lowlink = state.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if state.on_stack.contains(ref_name) {
                    let ref_index = state.indices[ref_name];
                    let my_lowlink = state.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_index);
                }
            }
        }

        if state.lowlinks[name] == state.indices[name] {
            let mut scc = Vec::new();
            loop {
                let w = state.stack.pop().unwrap();
                state.on_stack.swap_remove(&w);
                scc.push(w.clone());
                if w == name {
                    break;
                }
            }
            state.sccs.push(scc);
        }
    }

    let mut state = State {
        symbols,
        index: 0,
        stack: Vec::new(),
        on_stack: IndexSet::new(),
        indices: IndexMap::new(),
        lowlinks: IndexMap::new(),
        sccs: Vec::new(),
    };

    for name in symbols.names() {
        if !state.indices.contains_key(name) {
            strongconnect(name, &mut state);
        }
    }

    state
        .sccs
        .into_iter()
        .filter(|scc| {
            scc.len() > 1
                || symbols
                    .get(&scc[0])
                    .map(|d| d.refs.contains(&scc[0]))
                    .unwrap_or(false)
        })
        .collect()
}

fn find_def_by_name(root: &Root, name: &str) -> Option<Def> {
    root.defs()
        .find(|d| d.name().map(|n| n.text() == name).unwrap_or(false))
}

fn find_reference_location(root: &Root, from: &str, to: &str) -> Option<TextRange> {
    let def = find_def_by_name(root, from)?;
    let body = def.body()?;
    find_ref_in_expr(&body, to)
}

fn find_ref_in_expr(expr: &Expr, target: &str) -> Option<TextRange> {
    match expr {
        Expr::Ref(r) => {
            if let Some(name_token) = r.name() {
                if name_token.text() == target {
                    return Some(name_token.text_range());
                }
            }
            None
        }
        Expr::Tree(tree) => tree
            .children()
            .find_map(|child| find_ref_in_expr(&child, target)),
        Expr::Alt(alt) => alt
            .branches()
            .find_map(|b| b.body().and_then(|body| find_ref_in_expr(&body, target)))
            .or_else(|| alt.exprs().find_map(|e| find_ref_in_expr(&e, target))),
        Expr::Seq(seq) => seq.children().find_map(|c| find_ref_in_expr(&c, target)),
        Expr::Capture(cap) => cap
            .inner()
            .and_then(|inner| find_ref_in_expr(&inner, target)),
        Expr::Quantifier(q) => q.inner().and_then(|inner| find_ref_in_expr(&inner, target)),
        Expr::Field(f) => f.value().and_then(|v| find_ref_in_expr(&v, target)),
        _ => None,
    }
}

fn build_self_ref_chain(root: &Root, name: &str) -> Vec<RelatedInfo> {
    find_reference_location(root, name, name)
        .map(|range| {
            vec![RelatedInfo::new(
                range,
                format!("`{}` references itself", name),
            )]
        })
        .unwrap_or_default()
}

fn build_cycle_chain(root: &Root, symbols: &SymbolTable, scc: &[String]) -> Vec<RelatedInfo> {
    let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();
    let mut visited = IndexSet::new();
    let mut path = Vec::new();
    let start = &scc[0];

    fn find_path(
        current: &str,
        start: &str,
        scc_set: &IndexSet<&str>,
        symbols: &SymbolTable,
        visited: &mut IndexSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if visited.contains(current) {
            return current == start && path.len() > 1;
        }
        visited.insert(current.to_string());
        path.push(current.to_string());

        if let Some(def_info) = symbols.get(current) {
            for ref_name in &def_info.refs {
                if scc_set.contains(ref_name.as_str())
                    && find_path(ref_name, start, scc_set, symbols, visited, path)
                {
                    return true;
                }
            }
        }

        path.pop();
        false
    }

    find_path(start, start, &scc_set, symbols, &mut visited, &mut path);

    path.iter()
        .enumerate()
        .filter_map(|(i, from)| {
            let to = &path[(i + 1) % path.len()];
            find_reference_location(root, from, to).map(|range| {
                let msg = if i == path.len() - 1 {
                    format!("`{}` references `{}` (completing cycle)", from, to)
                } else {
                    format!("`{}` references `{}`", from, to)
                };
                RelatedInfo::new(range, msg)
            })
        })
        .collect()
}

fn make_error(primary_name: &str, scc: &[String], related: Vec<RelatedInfo>) -> SyntaxError {
    let cycle_str = if scc.len() == 1 {
        format!("`{}` → `{}`", primary_name, primary_name)
    } else {
        let mut cycle: Vec<_> = scc.iter().map(|s| format!("`{}`", s)).collect();
        cycle.push(format!("`{}`", scc[0]));
        cycle.join(" → ")
    };

    let range = related
        .first()
        .map(|r| r.range)
        .unwrap_or_else(|| TextRange::empty(0.into()));

    SyntaxError::with_related_many(
        range,
        format!(
            "recursive pattern can never match: cycle {} has no escape path",
            cycle_str
        ),
        related,
    )
    .with_stage(ErrorStage::Escape)
}

#[cfg(test)]
mod tests {
    use crate::Query;
    use indoc::indoc;

    #[test]
    fn self_recursion_no_escape() {
        let query = Query::new("Expr = (call (Expr))");
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.render_errors(), @r"
        error: recursive pattern can never match: cycle `Expr` → `Expr` has no escape path
          |
        1 | Expr = (call (Expr))
          |               ^^^^
          |               |
          |               recursive pattern can never match: cycle `Expr` → `Expr` has no escape path
          |               `Expr` references itself
        ");
    }

    #[test]
    fn self_recursion_with_escape_via_alternation() {
        let query = Query::new("Expr = [(identifier) (call (Expr))]");
        assert!(query.is_valid());
    }

    #[test]
    fn self_recursion_with_escape_via_optional() {
        let query = Query::new("Expr = (call (Expr)?)");
        assert!(query.is_valid());
    }

    #[test]
    fn self_recursion_with_escape_via_star() {
        let query = Query::new("Expr = (call (Expr)*)");
        assert!(query.is_valid());
    }

    #[test]
    fn self_recursion_no_escape_via_plus() {
        let query = Query::new("Expr = (call (Expr)+)");
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.render_errors(), @r"
        error: recursive pattern can never match: cycle `Expr` → `Expr` has no escape path
          |
        1 | Expr = (call (Expr)+)
          |               ^^^^
          |               |
          |               recursive pattern can never match: cycle `Expr` → `Expr` has no escape path
          |               `Expr` references itself
        ");
    }

    #[test]
    fn mutual_recursion_no_escape() {
        let input = indoc! {r#"
            A = (foo (B))
            B = (bar (A))
        "#};
        let query = Query::new(input);
        assert!(!query.is_valid());
        insta::assert_snapshot!(query.render_errors(), @r"
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
    fn mutual_recursion_with_escape() {
        let input = indoc! {r#"
            A = [(x) (foo (B))]
            B = [(y) (bar (A))]
        "#};
        let query = Query::new(input);
        assert!(query.is_valid());
    }

    #[test]
    fn three_way_cycle_no_escape() {
        let input = indoc! {r#"
            A = (a (B))
            B = (b (C))
            C = (c (A))
        "#};
        let query = Query::new(input);
        assert!(!query.is_valid());
        assert!(
            query
                .render_errors()
                .contains("recursive pattern can never match")
        );
    }

    #[test]
    fn non_recursive_reference() {
        let input = indoc! {r#"
            Leaf = (identifier)
            Tree = (call (Leaf))
        "#};
        let query = Query::new(input);
        assert!(query.is_valid());
    }

    #[test]
    fn reference_from_entry_point() {
        let input = indoc! {r#"
            Expr = [(identifier) (call (Expr))]
            (program (Expr))
        "#};
        let query = Query::new(input);
        assert!(query.is_valid());
    }
}
