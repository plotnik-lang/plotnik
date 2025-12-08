//! Escape path analysis for recursive definitions.
//!
//! Detects patterns that can never match because they require
//! infinitely nested structures (recursion with no escape path),
//! or infinite runtime loops where the cursor never advances (left recursion).

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

            // 1. Check for infinite tree structure (Escape Analysis)
            // Existing logic: at least one definition must have a non-recursive path.
            let has_escape = scc.iter().any(|name| {
                self.symbol_table
                    .get(name.as_str())
                    .map(|body| expr_has_escape(body, &scc_set))
                    .unwrap_or(true)
            });

            if !has_escape {
                let chain = if scc.len() == 1 {
                    self.build_self_ref_chain(&scc[0])
                } else {
                    self.build_cycle_chain(&scc)
                };
                self.emit_recursion_error(&scc[0], &scc, chain);
                continue;
            }

            // 2. Check for infinite loops (Guarded Recursion Analysis)
            // Ensure every recursive cycle consumes at least one node.
            if let Some(cycle) = self.find_unguarded_cycle(&scc, &scc_set) {
                let chain = self.build_unguarded_chain(&cycle);
                self.emit_direct_recursion_error(&cycle[0], &cycle, chain);
            }
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

    fn find_unguarded_cycle(
        &self,
        scc: &[String],
        scc_set: &IndexSet<&str>,
    ) -> Option<Vec<String>> {
        // Build dependency graph for unguarded calls within the SCC
        let mut adj = IndexMap::new();
        for name in scc {
            if let Some(body) = self.symbol_table.get(name.as_str()) {
                let mut refs = IndexSet::new();
                collect_unguarded_refs(body, scc_set, &mut refs);
                adj.insert(name.clone(), refs);
            }
        }

        // Detect cycle
        let mut visited = IndexSet::new();
        let mut stack = IndexSet::new();

        for start_node in scc {
            if let Some(target) = Self::detect_cycle(start_node, &adj, &mut visited, &mut stack) {
                let index = stack.get_index_of(&target).unwrap();
                return Some(stack.iter().skip(index).cloned().collect());
            }
        }

        None
    }

    fn detect_cycle(
        node: &String,
        adj: &IndexMap<String, IndexSet<String>>,
        visited: &mut IndexSet<String>,
        stack: &mut IndexSet<String>,
    ) -> Option<String> {
        if stack.contains(node) {
            return Some(node.clone());
        }
        if visited.contains(node) {
            return None;
        }

        visited.insert(node.clone());
        stack.insert(node.clone());

        if let Some(neighbors) = adj.get(node) {
            for neighbor in neighbors {
                if let Some(target) = Self::detect_cycle(neighbor, adj, visited, stack) {
                    return Some(target);
                }
            }
        }

        stack.pop();
        None
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

    fn find_unguarded_reference_location(&self, from: &str, to: &str) -> Option<TextRange> {
        let def = self.find_def_by_name(from)?;
        let body = def.body()?;
        find_unguarded_ref_in_expr(&body, to)
    }

    fn build_self_ref_chain(&self, name: &str) -> Vec<(TextRange, String)> {
        self.find_reference_location(name, name)
            .map(|range| vec![(range, format!("`{}` references itself", name))])
            .unwrap_or_default()
    }

    fn build_cycle_chain(&self, scc: &[String]) -> Vec<(TextRange, String)> {
        // Since Tarjan's sccs are not guaranteed to be ordered as a cycle,
        // we need to find the cycle path explicitly.
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

    fn build_unguarded_chain(&self, cycle: &[String]) -> Vec<(TextRange, String)> {
        if cycle.len() == 1 {
            return self
                .find_unguarded_reference_location(&cycle[0], &cycle[0])
                .map(|range| vec![(range, format!("`{}` references itself", cycle[0]))])
                .unwrap_or_default();
        }
        self.build_chain_generic(cycle, |from, to| {
            self.find_unguarded_reference_location(from, to)
        })
    }

    fn build_chain_generic<F>(&self, path_nodes: &[String], find_loc: F) -> Vec<(TextRange, String)>
    where
        F: Fn(&str, &str) -> Option<TextRange>,
    {
        path_nodes
            .iter()
            .enumerate()
            .filter_map(|(i, from)| {
                let to = &path_nodes[(i + 1) % path_nodes.len()];
                find_loc(from, to).map(|range| {
                    let msg = if i == path_nodes.len() - 1 {
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

        let def_range = if scc.len() > 1 {
            self.find_def_by_name(primary_name)
                .and_then(|def| def.name())
                .map(|n| n.text_range())
        } else {
            None
        };

        let mut builder = self
            .recursion_diagnostics
            .report(DiagnosticKind::RecursionNoEscape, range)
            .message(format!("cycle {} has no escape path", cycle_str));

        for (rel_range, rel_msg) in related {
            builder = builder.related_to(rel_msg, rel_range);
        }

        if let Some(range) = def_range {
            builder = builder.related_to(format!("`{}` is defined here", primary_name), range);
        }

        builder.emit();
    }
    fn emit_direct_recursion_error(
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

        let def_range = if scc.len() > 1 {
            self.find_def_by_name(primary_name)
                .and_then(|def| def.name())
                .map(|n| n.text_range())
        } else {
            None
        };

        let mut builder = self
            .recursion_diagnostics
            .report(DiagnosticKind::DirectRecursion, range)
            .message(format!(
                "cycle {} will stuck without matching anything",
                cycle_str
            ));

        for (rel_range, rel_msg) in related {
            builder = builder.related_to(rel_msg, rel_range);
        }

        if let Some(range) = def_range {
            builder = builder.related_to(format!("`{}` is defined here", primary_name), range);
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

fn expr_guarantees_consumption(expr: &Expr) -> bool {
    match expr {
        Expr::NamedNode(_) | Expr::AnonymousNode(_) => true,
        Expr::Ref(_) => false,
        Expr::AltExpr(_) => expr.children().iter().all(expr_guarantees_consumption),
        Expr::SeqExpr(_) => expr.children().iter().any(expr_guarantees_consumption),
        Expr::QuantifiedExpr(q) => {
            !q.is_optional()
                && q.inner()
                    .map(|i| expr_guarantees_consumption(&i))
                    .unwrap_or(false)
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
            expr.children().iter().all(expr_guarantees_consumption)
        }
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

fn collect_unguarded_refs(expr: &Expr, scc: &IndexSet<&str>, refs: &mut IndexSet<String>) {
    match expr {
        Expr::Ref(r) => {
            if let Some(name) = r.name().filter(|n| scc.contains(n.text())) {
                refs.insert(name.text().to_string());
            }
        }
        Expr::NamedNode(_) | Expr::AnonymousNode(_) => {
            // Consumes input, so guards recursion. Do not collect refs inside.
        }
        Expr::AltExpr(_) => {
            for c in expr.children() {
                collect_unguarded_refs(&c, scc, refs);
            }
        }
        Expr::SeqExpr(_) => {
            for c in expr.children() {
                collect_unguarded_refs(&c, scc, refs);
                if expr_guarantees_consumption(&c) {
                    break;
                }
            }
        }
        Expr::QuantifiedExpr(q) => {
            if let Some(inner) = q.inner() {
                collect_unguarded_refs(&inner, scc, refs);
            }
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
            for c in expr.children() {
                collect_unguarded_refs(&c, scc, refs);
            }
        }
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

fn find_unguarded_ref_in_expr(expr: &Expr, target: &str) -> Option<TextRange> {
    match expr {
        Expr::Ref(r) => r
            .name()
            .filter(|n| n.text() == target)
            .map(|n| n.text_range()),
        Expr::NamedNode(_) | Expr::AnonymousNode(_) => None,
        Expr::AltExpr(_) => expr
            .children()
            .iter()
            .find_map(|c| find_unguarded_ref_in_expr(c, target)),
        Expr::SeqExpr(_) => {
            for c in expr.children() {
                if let Some(range) = find_unguarded_ref_in_expr(&c, target) {
                    return Some(range);
                }
                if expr_guarantees_consumption(&c) {
                    return None;
                }
            }
            None
        }
        Expr::QuantifiedExpr(q) => q
            .inner()
            .and_then(|i| find_unguarded_ref_in_expr(&i, target)),
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => expr
            .children()
            .iter()
            .find_map(|c| find_unguarded_ref_in_expr(c, target)),
    }
}
