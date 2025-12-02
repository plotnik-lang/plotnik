use std::fmt::Write;

use crate::ast::{self, Root, SyntaxNode};

use super::Query;
use super::shape_cardinalities::ShapeCardinality;

impl Query<'_> {
    pub fn dump_cst(&self) -> String {
        let mut out = String::new();
        dump_tree(&self.syntax(), 0, &mut out, false);
        out
    }

    pub fn dump_cst_full(&self) -> String {
        let mut out = String::new();
        dump_tree(&self.syntax(), 0, &mut out, true);
        out
    }

    pub fn dump_ast(&self) -> String {
        ast::format_ast(&self.root())
    }

    pub fn dump_shapes(&self) -> String {
        let mut out = String::new();
        self.dump_shape_root(&self.root(), &mut out);
        out
    }

    pub fn dump_symbols(&self) -> String {
        let mut out = String::new();

        let mut defs: Vec<_> = self.symbols.iter().collect();
        defs.sort_by_key(|d| &d.name);

        for def in &defs {
            out.push_str(&def.name);
            if !def.refs.is_empty() {
                let mut refs: Vec<_> = def.refs.iter().map(|s| s.as_str()).collect();
                refs.sort();
                out.push_str(" -> ");
                out.push_str(&refs.join(", "));
            }
            out.push('\n');
        }

        out
    }

    fn dump_shape_root(&self, root: &Root, out: &mut String) {
        let card = self.shape_cardinality(root.syntax());
        let mark = cardinality_mark(card);
        let _ = writeln!(out, "Root{}", mark);
        for def in root.defs() {
            self.dump_shape_def(&def, 1, out);
        }
        for expr in root.exprs() {
            self.dump_shape_expr(&expr, 1, out);
        }
    }

    fn dump_shape_def(&self, def: &ast::Def, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(def.syntax());
        let mark = cardinality_mark(card);
        let name = def.name().map(|t| t.text().to_string());
        match name {
            Some(n) => {
                let _ = writeln!(out, "{}Def{} {}", prefix, mark, n);
            }
            None => {
                let _ = writeln!(out, "{}Def{}", prefix, mark);
            }
        }
        if let Some(body) = def.body() {
            self.dump_shape_expr(&body, indent + 1, out);
        }
    }

    fn dump_shape_expr(&self, expr: &ast::Expr, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(expr.syntax());
        let mark = cardinality_mark(card);

        match expr {
            ast::Expr::Tree(t) => {
                let node_type = t.node_type().map(|tok| tok.text().to_string());
                match node_type {
                    Some(ty) => {
                        let _ = writeln!(out, "{}Tree{} {}", prefix, mark, ty);
                    }
                    None => {
                        let _ = writeln!(out, "{}Tree{}", prefix, mark);
                    }
                }
                for child in t.children() {
                    self.dump_shape_expr(&child, indent + 1, out);
                }
            }
            ast::Expr::Ref(r) => {
                let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}Ref{} {}", prefix, mark, name);
            }
            ast::Expr::Lit(l) => {
                let value = l.value().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}Lit{} {}", prefix, mark, value);
            }
            ast::Expr::Str(s) => {
                let value = s.value().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}Str{} \"{}\"", prefix, mark, value);
            }
            ast::Expr::Alt(a) => {
                let _ = writeln!(out, "{}Alt{}", prefix, mark);
                for branch in a.branches() {
                    self.dump_shape_branch(&branch, indent + 1, out);
                }
                for expr in a.exprs() {
                    self.dump_shape_expr(&expr, indent + 1, out);
                }
            }
            ast::Expr::Seq(s) => {
                let _ = writeln!(out, "{}Seq{}", prefix, mark);
                for child in s.children() {
                    self.dump_shape_expr(&child, indent + 1, out);
                }
            }
            ast::Expr::Capture(c) => {
                let name = c.name().map(|t| t.text().to_string()).unwrap_or_default();
                let type_ann = c
                    .type_annotation()
                    .and_then(|t| t.name())
                    .map(|t| t.text().to_string());
                match type_ann {
                    Some(ty) => {
                        let _ = writeln!(out, "{}Capture{} @{} :: {}", prefix, mark, name, ty);
                    }
                    None => {
                        let _ = writeln!(out, "{}Capture{} @{}", prefix, mark, name);
                    }
                }
                if let Some(inner) = c.inner() {
                    self.dump_shape_expr(&inner, indent + 1, out);
                }
            }
            ast::Expr::Quantifier(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                let _ = writeln!(out, "{}Quantifier{} {}", prefix, mark, op);
                if let Some(inner) = q.inner() {
                    self.dump_shape_expr(&inner, indent + 1, out);
                }
            }
            ast::Expr::Field(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}Field{} {}:", prefix, mark, name);
                if let Some(value) = f.value() {
                    self.dump_shape_expr(&value, indent + 1, out);
                }
            }
            ast::Expr::NegatedField(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}NegatedField{} !{}", prefix, mark, name);
            }
            ast::Expr::Wildcard(_) => {
                let _ = writeln!(out, "{}Wildcard{}", prefix, mark);
            }
            ast::Expr::Anchor(_) => {
                let _ = writeln!(out, "{}Anchor{}", prefix, mark);
            }
        }
    }

    fn dump_shape_branch(&self, branch: &ast::Branch, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(branch.syntax());
        let mark = cardinality_mark(card);
        let label = branch.label().map(|t| t.text().to_string());
        match label {
            Some(l) => {
                let _ = writeln!(out, "{}Branch{} {}:", prefix, mark, l);
            }
            None => {
                let _ = writeln!(out, "{}Branch{}", prefix, mark);
            }
        }
        if let Some(body) = branch.body() {
            self.dump_shape_expr(&body, indent + 1, out);
        }
    }
}

fn dump_tree(node: &SyntaxNode, indent: usize, out: &mut String, include_trivia: bool) {
    let prefix = "  ".repeat(indent);
    let _ = writeln!(out, "{}{:?}", prefix, node.kind());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => dump_tree(&n, indent + 1, out, include_trivia),
            rowan::NodeOrToken::Token(t) => {
                if include_trivia || !t.kind().is_trivia() {
                    let child_prefix = "  ".repeat(indent + 1);
                    let _ = writeln!(out, "{}{:?} {:?}", child_prefix, t.kind(), t.text());
                }
            }
        }
    }
}

fn cardinality_mark(card: ShapeCardinality) -> &'static str {
    match card {
        ShapeCardinality::One => "¹",
        ShapeCardinality::Many => "⁺",
    }
}
