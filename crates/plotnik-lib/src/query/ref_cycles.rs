//! Escape path analysis for recursive definitions.
//!
//! Detects patterns that can never match because they require
//! infinitely nested structures (recursion with no escape path).

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::named_defs::SymbolTable;
use crate::PassResult;
use crate::diagnostics::Diagnostics;
use crate::parser::{Def, Expr, Root, SyntaxKind};

pub fn validate(root: &Root, symbols: &SymbolTable<'_>) -> PassResult<()> {
    let sccs = find_sccs(symbols);
    let mut errors = Diagnostics::new();

    for scc in sccs {
        if scc.len() == 1 {
            let name = &scc[0];
            let Some(body) = symbols.get(name.as_str()) else {
                continue;
            };

            let refs = collect_refs(body);
            if !refs.contains(name) {
                continue;
            }

            let scc_set: IndexSet<&str> = std::iter::once(name.as_str()).collect();
            if !expr_has_escape(body, &scc_set) {
                let chain = build_self_ref_chain(root, name);
                emit_error(&mut errors, name, &scc, chain);
            }
            continue;
        }

        let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();
        let mut any_has_escape = false;

        for name in &scc {
            if let Some(def) = find_def_by_name(root, name)
                && let Some(body) = def.body()
                && expr_has_escape(&body, &scc_set)
            {
                any_has_escape = true;
                break;
            }
        }

        if !any_has_escape {
            let chain = build_cycle_chain(root, symbols, &scc);
            emit_error(&mut errors, &scc[0], &scc, chain);
        }
    }

    Ok(((), errors))
}

fn expr_has_escape(expr: &Expr, scc: &IndexSet<&str>) -> bool {
    match expr {
        Expr::Ref(r) => {
            // A Ref is always a reference to a user-defined expression
            // If it's in the SCC, it doesn't provide an escape path
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc.contains(name_token.text())
        }
        Expr::NamedNode(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc))
        }

        Expr::AltExpr(alt) => {
            alt.branches().any(|b| {
                b.body()
                    .map(|body| expr_has_escape(&body, scc))
                    .unwrap_or(true)
            }) || alt.exprs().any(|e| expr_has_escape(&e, scc))
        }

        Expr::SeqExpr(seq) => seq.children().all(|c| expr_has_escape(&c, scc)),

        Expr::QuantifiedExpr(q) => match q.operator().map(|op| op.kind()) {
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

        Expr::CapturedExpr(cap) => cap
            .inner()
            .map(|inner| expr_has_escape(&inner, scc))
            .unwrap_or(true),

        Expr::FieldExpr(f) => f.value().map(|v| expr_has_escape(&v, scc)).unwrap_or(true),

        Expr::AnonymousNode(_) => true,
    }
}

fn collect_refs(expr: &Expr) -> IndexSet<String> {
    let mut refs = IndexSet::new();
    collect_refs_into(expr, &mut refs);
    refs
}

fn collect_refs_into(expr: &Expr, refs: &mut IndexSet<String>) {
    match expr {
        Expr::Ref(r) => {
            let Some(name_token) = r.name() else { return };
            refs.insert(name_token.text().to_string());
        }
        Expr::NamedNode(node) => {
            for child in node.children() {
                collect_refs_into(&child, refs);
            }
        }
        Expr::AltExpr(alt) => {
            for branch in alt.branches() {
                let Some(body) = branch.body() else { continue };
                collect_refs_into(&body, refs);
            }
        }
        Expr::SeqExpr(seq) => {
            for child in seq.children() {
                collect_refs_into(&child, refs);
            }
        }
        Expr::CapturedExpr(cap) => {
            let Some(inner) = cap.inner() else { return };
            collect_refs_into(&inner, refs);
        }
        Expr::QuantifiedExpr(q) => {
            let Some(inner) = q.inner() else { return };
            collect_refs_into(&inner, refs);
        }
        Expr::FieldExpr(f) => {
            let Some(value) = f.value() else { return };
            collect_refs_into(&value, refs);
        }
        Expr::AnonymousNode(_) => {}
    }
}

fn find_sccs(symbols: &SymbolTable<'_>) -> Vec<Vec<String>> {
    struct State<'a> {
        symbols: &'a SymbolTable<'a>,
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

        if let Some(body) = state.symbols.get(name) {
            let refs = collect_refs(body);
            for ref_name in &refs {
                if state.symbols.get(ref_name.as_str()).is_none() {
                    continue;
                }
                if !state.indices.contains_key(ref_name.as_str()) {
                    strongconnect(ref_name, state);
                    let ref_lowlink = state.lowlinks[ref_name.as_str()];
                    let my_lowlink = state.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if state.on_stack.contains(ref_name.as_str()) {
                    let ref_index = state.indices[ref_name.as_str()];
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

    for name in symbols.keys() {
        if !state.indices.contains_key(*name) {
            strongconnect(name, &mut state);
        }
    }

    state
        .sccs
        .into_iter()
        .filter(|scc| {
            scc.len() > 1
                || symbols
                    .get(scc[0].as_str())
                    .map(|body| collect_refs(body).contains(scc[0].as_str()))
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
            let name_token = r.name()?;
            if name_token.text() == target {
                Some(name_token.text_range())
            } else {
                None
            }
        }
        Expr::NamedNode(node) => node
            .children()
            .find_map(|child| find_ref_in_expr(&child, target)),
        Expr::AltExpr(alt) => alt
            .branches()
            .find_map(|b| b.body().and_then(|body| find_ref_in_expr(&body, target)))
            .or_else(|| alt.exprs().find_map(|e| find_ref_in_expr(&e, target))),
        Expr::SeqExpr(seq) => seq.children().find_map(|c| find_ref_in_expr(&c, target)),
        Expr::CapturedExpr(cap) => cap
            .inner()
            .and_then(|inner| find_ref_in_expr(&inner, target)),
        Expr::QuantifiedExpr(q) => q.inner().and_then(|inner| find_ref_in_expr(&inner, target)),
        Expr::FieldExpr(f) => f.value().and_then(|v| find_ref_in_expr(&v, target)),
        _ => None,
    }
}

fn build_self_ref_chain(root: &Root, name: &str) -> Vec<(TextRange, String)> {
    find_reference_location(root, name, name)
        .map(|range| vec![(range, format!("`{}` references itself", name))])
        .unwrap_or_default()
}

fn build_cycle_chain(
    root: &Root,
    symbols: &SymbolTable<'_>,
    scc: &[String],
) -> Vec<(TextRange, String)> {
    let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();
    let mut visited = IndexSet::new();
    let mut path = Vec::new();
    let start = &scc[0];

    fn find_path(
        current: &str,
        start: &str,
        scc_set: &IndexSet<&str>,
        symbols: &SymbolTable<'_>,
        visited: &mut IndexSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if visited.contains(current) {
            return current == start && path.len() > 1;
        }
        visited.insert(current.to_string());
        path.push(current.to_string());

        if let Some(body) = symbols.get(current) {
            let refs = collect_refs(body);
            for ref_name in &refs {
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
                (range, msg)
            })
        })
        .collect()
}

fn emit_error(
    errors: &mut Diagnostics,
    primary_name: &str,
    scc: &[String],
    related: Vec<(TextRange, String)>,
) {
    let cycle_str = if scc.len() == 1 {
        format!("`{}` → `{}`", primary_name, primary_name)
    } else {
        let mut cycle: Vec<_> = scc.iter().map(|s| format!("`{}`", s)).collect();
        cycle.push(format!("`{}`", scc[0]));
        cycle.join(" → ")
    };

    let range = related
        .first()
        .map(|(r, _)| *r)
        .unwrap_or_else(|| TextRange::empty(0.into()));

    let mut builder = errors.error(
        format!(
            "recursive pattern can never match: cycle {} has no escape path",
            cycle_str
        ),
        range,
    );

    for (rel_range, rel_msg) in related {
        builder = builder.related_to(rel_msg, rel_range);
    }

    builder.emit();
}
