//! Escape path analysis for recursive definitions.
//!
//! Detects patterns that can never match because they require
//! infinitely nested structures (recursion with no escape path).

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::Query;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{Def, Expr};

impl Query<'_> {
    pub(super) fn validate_recursion(&mut self) {
        let sccs = self.find_sccs();

        for scc in sccs {
            let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();

            let has_escape = scc.iter().any(|name| {
                self.symbol_table
                    .get(name.as_str())
                    .map(|body| expr_has_escape(body, &scc_set))
                    .unwrap_or(true)
            });

            if has_escape {
                continue;
            }

            let chain = if scc.len() == 1 {
                self.build_self_ref_chain(&scc[0])
            } else {
                self.build_cycle_chain(&scc)
            };
            self.emit_recursion_error(&scc[0], &scc, chain);
        }
    }

    fn find_sccs(&self) -> Vec<Vec<String>> {
        struct State<'a, 'src> {
            query: &'a Query<'src>,
            index: usize,
            stack: Vec<String>,
            on_stack: IndexSet<String>,
            indices: IndexMap<String, usize>,
            lowlinks: IndexMap<String, usize>,
            sccs: Vec<Vec<String>>,
        }

        fn strongconnect(name: &str, state: &mut State<'_, '_>) {
            state.indices.insert(name.to_string(), state.index);
            state.lowlinks.insert(name.to_string(), state.index);
            state.index += 1;
            state.stack.push(name.to_string());
            state.on_stack.insert(name.to_string());

            if let Some(body) = state.query.symbol_table.get(name) {
                let refs = collect_refs(body);
                for ref_name in &refs {
                    if state.query.symbol_table.get(ref_name.as_str()).is_none() {
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
            query: self,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
        };

        for name in self.symbol_table.keys() {
            if !state.indices.contains_key(*name) {
                strongconnect(name, &mut state);
            }
        }

        state
            .sccs
            .into_iter()
            .filter(|scc| {
                scc.len() > 1
                    || self
                        .symbol_table
                        .get(scc[0].as_str())
                        .map(|body| collect_refs(body).contains(scc[0].as_str()))
                        .unwrap_or(false)
            })
            .collect()
    }

    fn find_def_by_name(&self, name: &str) -> Option<Def> {
        self.ast
            .defs()
            .find(|d| d.name().map(|n| n.text() == name).unwrap_or(false))
    }

    fn find_reference_location(&self, from: &str, to: &str) -> Option<TextRange> {
        let def = self.find_def_by_name(from)?;
        let body = def.body()?;
        find_ref_in_expr(&body, to)
    }

    fn build_self_ref_chain(&self, name: &str) -> Vec<(TextRange, String)> {
        self.find_reference_location(name, name)
            .map(|range| vec![(range, format!("`{}` references itself", name))])
            .unwrap_or_default()
    }

    fn build_cycle_chain(&self, scc: &[String]) -> Vec<(TextRange, String)> {
        let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();
        let mut visited = IndexSet::new();
        let mut path = Vec::new();
        let start = &scc[0];

        fn find_path<'a>(
            current: &str,
            start: &str,
            scc_set: &IndexSet<&str>,
            query: &Query<'a>,
            visited: &mut IndexSet<String>,
            path: &mut Vec<String>,
        ) -> bool {
            if visited.contains(current) {
                return current == start && path.len() > 1;
            }
            visited.insert(current.to_string());
            path.push(current.to_string());

            if let Some(body) = query.symbol_table.get(current) {
                let refs = collect_refs(body);
                for ref_name in &refs {
                    if scc_set.contains(ref_name.as_str())
                        && find_path(ref_name, start, scc_set, query, visited, path)
                    {
                        return true;
                    }
                }
            }

            path.pop();
            false
        }

        find_path(start, start, &scc_set, self, &mut visited, &mut path);

        path.iter()
            .enumerate()
            .filter_map(|(i, from)| {
                let to = &path[(i + 1) % path.len()];
                self.find_reference_location(from, to).map(|range| {
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

    fn emit_recursion_error(
        &mut self,
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

        let mut builder = self
            .recursion_diagnostics
            .report(DiagnosticKind::RecursionNoEscape, range)
            .message(format!("cycle {} has no escape path", cycle_str));

        for (rel_range, rel_msg) in related {
            builder = builder.related_to(rel_msg, rel_range);
        }

        builder.emit();
    }
}

fn expr_has_escape(expr: &Expr, scc: &IndexSet<&str>) -> bool {
    match expr {
        Expr::Ref(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc.contains(name_token.text())
        }
        Expr::NamedNode(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc))
        }
        Expr::AltExpr(_) => expr.children().iter().any(|c| expr_has_escape(c, scc)),
        Expr::SeqExpr(_) => expr.children().iter().all(|c| expr_has_escape(c, scc)),
        Expr::QuantifiedExpr(q) => {
            if q.is_optional() {
                return true;
            }
            q.inner()
                .map(|inner| expr_has_escape(&inner, scc))
                .unwrap_or(true)
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
            expr.children().iter().all(|c| expr_has_escape(c, scc))
        }
        Expr::AnonymousNode(_) => true,
    }
}

fn collect_refs(expr: &Expr) -> IndexSet<String> {
    let mut refs = IndexSet::new();
    collect_refs_into(expr, &mut refs);
    refs
}

fn collect_refs_into(expr: &Expr, refs: &mut IndexSet<String>) {
    if let Expr::Ref(r) = expr
        && let Some(name_token) = r.name()
    {
        refs.insert(name_token.text().to_string());
    }

    for child in expr.children() {
        collect_refs_into(&child, refs);
    }
}

fn find_ref_in_expr(expr: &Expr, target: &str) -> Option<TextRange> {
    if let Expr::Ref(r) = expr {
        let name_token = r.name()?;
        if name_token.text() == target {
            return Some(name_token.text_range());
        }
    }

    expr.children()
        .iter()
        .find_map(|child| find_ref_in_expr(child, target))
}
