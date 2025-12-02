//! Plotnik: Query language for tree-sitter AST with type inference.
//!
//! # Example
//!
//! ```
//! use plotnik_lib::Query;
//!
//! let query = Query::new(r#"
//!     Expr = [(identifier) (number)]
//!     (assignment left: (Expr) @lhs right: (Expr) @rhs)
//! "#);
//!
//! if !query.is_valid() {
//!     eprintln!("{}", query.render_errors());
//! }
//! ```

pub mod ast;
pub mod escape;
pub mod lexer;
pub mod parser;
pub mod resolve;
pub mod shape_cardinality;
pub mod syntax_kind;
pub mod validate;

#[cfg(test)]
mod ast_tests;
#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod shape_cardinality_tests;

use ast::{Root, format_ast};
use parser::{ErrorStage, Parse, SyntaxError};
use resolve::SymbolTable;
use shape_cardinality::ShapeCardinality;
use std::collections::HashMap;
use std::fmt::Write;
use syntax_kind::SyntaxNode;

/// A parsed and resolved query.
///
/// Construction always succeeds. Check [`is_valid`](Self::is_valid) or
/// [`errors`](Self::errors) to determine if the query is usable.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    source: &'a str,
    parse: Parse,
    symbols: SymbolTable,
    errors: Vec<SyntaxError>,
    shape_cardinality: HashMap<SyntaxNode, ShapeCardinality>,
}

impl<'a> Query<'a> {
    /// Parse and resolve a query from source text.
    ///
    /// This never fails. Parse and resolution errors are collected
    /// and accessible via [`errors`](Self::errors).
    pub fn new(source: &'a str) -> Self {
        let parse = parser::parse(source);

        let root = Root::cast(parse.syntax()).expect("parser always produces Root");
        let resolve_result = resolve::resolve(&root);

        let mut errors = parse.errors().to_vec();

        // Semantic validation (mixed alternations, etc.)
        let validate_errors = validate::validate(&root);
        errors.extend(validate_errors);

        errors.extend(resolve_result.errors);

        // Check for recursive patterns with no escape path
        let escape_errors = escape::check_escape(&root, &resolve_result.symbols);
        errors.extend(escape_errors);

        // Shape analysis (only on valid ASTs - relies on invariants from earlier phases)
        let shape_cardinality = if errors.is_empty() {
            let cards = shape_cardinality::compute_cardinalities(&root, &resolve_result.symbols);
            let shape_errors =
                shape_cardinality::validate_shapes(&root, &resolve_result.symbols, &cards);
            errors.extend(shape_errors);
            cards
        } else {
            HashMap::new()
        };

        Self {
            source,
            parse,
            symbols: resolve_result.symbols,
            errors,
            shape_cardinality,
        }
    }

    /// The original source text.
    pub fn source(&self) -> &str {
        self.source
    }

    /// The concrete syntax tree root.
    pub fn syntax(&self) -> SyntaxNode {
        self.parse.syntax()
    }

    /// The typed AST root.
    pub fn root(&self) -> Root {
        Root::cast(self.parse.syntax()).expect("parser always produces Root")
    }

    /// Symbol table with all named definitions and their references.
    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }

    /// Get shape cardinality for an expression node.
    pub fn shape_cardinality(&self, node: &SyntaxNode) -> ShapeCardinality {
        self.shape_cardinality
            .get(node)
            .copied()
            .unwrap_or(ShapeCardinality::One)
    }

    /// All errors from parsing and resolution.
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Returns `true` if the query has no errors.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Render errors as a human-readable diagnostic report.
    pub fn render_errors(&self) -> String {
        parser::render_errors(self.source, &self.errors, None)
    }

    /// Filter errors by stage.
    pub fn errors_by_stage(&self, stage: ErrorStage) -> Vec<&SyntaxError> {
        self.errors.iter().filter(|e| e.stage == stage).collect()
    }

    /// Returns `true` if there are parse-stage errors.
    pub fn has_parse_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Parse)
    }

    /// Returns `true` if there are validate-stage errors.
    pub fn has_validate_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Validate)
    }

    /// Returns `true` if there are resolve-stage errors.
    pub fn has_resolve_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Resolve)
    }

    /// Returns `true` if there are escape-stage errors.
    pub fn has_escape_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Escape)
    }

    /// Render errors for a specific stage.
    pub fn render_errors_by_stage(&self, stage: ErrorStage) -> String {
        let filtered: Vec<_> = self.errors_by_stage(stage).into_iter().cloned().collect();
        parser::render_errors(self.source, &filtered, None)
    }

    /// Render errors grouped by stage.
    pub fn render_errors_grouped(&self) -> String {
        let mut out = String::new();
        for stage in [
            ErrorStage::Parse,
            ErrorStage::Validate,
            ErrorStage::Resolve,
            ErrorStage::Escape,
        ] {
            let stage_errors: Vec<_> = self.errors_by_stage(stage).into_iter().cloned().collect();
            if !stage_errors.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("=== {} errors ===\n", stage));
                out.push_str(&parser::render_errors(self.source, &stage_errors, None));
            }
        }
        out
    }

    /// Format CST structure (without trivia, without errors).
    pub fn format_cst(&self) -> String {
        let mut out = String::new();
        Self::format_tree(&self.syntax(), 0, &mut out, false);
        out
    }

    /// Format CST structure (with trivia, without errors).
    pub fn format_cst_raw(&self) -> String {
        let mut out = String::new();
        Self::format_tree(&self.syntax(), 0, &mut out, true);
        out
    }

    /// Format AST structure (semantic tree without syntactic tokens).
    pub fn format_ast(&self) -> String {
        format_ast(&self.root())
    }

    /// Format AST with shape cardinality annotations.
    /// Uses `¹` for One (single node) and `⁺` for Many (sequence).
    pub fn format_shape_cardinality(&self) -> String {
        let mut out = String::new();
        self.format_shape_root(&self.root(), &mut out);
        out
    }

    fn format_shape_root(&self, root: &Root, out: &mut String) {
        let card = self.shape_cardinality(root.syntax());
        let mark = Self::cardinality_mark(card);
        let _ = writeln!(out, "Root{}", mark);
        for def in root.defs() {
            self.format_shape_def(&def, 1, out);
        }
        for expr in root.exprs() {
            self.format_shape_expr(&expr, 1, out);
        }
    }

    fn format_shape_def(&self, def: &ast::Def, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(def.syntax());
        let mark = Self::cardinality_mark(card);
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
            self.format_shape_expr(&body, indent + 1, out);
        }
    }

    fn format_shape_expr(&self, expr: &ast::Expr, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(expr.syntax());
        let mark = Self::cardinality_mark(card);

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
                    self.format_shape_expr(&child, indent + 1, out);
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
                    self.format_shape_branch(&branch, indent + 1, out);
                }
                for expr in a.exprs() {
                    self.format_shape_expr(&expr, indent + 1, out);
                }
            }
            ast::Expr::Seq(s) => {
                let _ = writeln!(out, "{}Seq{}", prefix, mark);
                for child in s.children() {
                    self.format_shape_expr(&child, indent + 1, out);
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
                    self.format_shape_expr(&inner, indent + 1, out);
                }
            }
            ast::Expr::Quantifier(q) => {
                let op = q
                    .operator()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                let _ = writeln!(out, "{}Quantifier{} {}", prefix, mark, op);
                if let Some(inner) = q.inner() {
                    self.format_shape_expr(&inner, indent + 1, out);
                }
            }
            ast::Expr::Field(f) => {
                let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
                let _ = writeln!(out, "{}Field{} {}:", prefix, mark, name);
                if let Some(value) = f.value() {
                    self.format_shape_expr(&value, indent + 1, out);
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

    fn format_shape_branch(&self, branch: &ast::Branch, indent: usize, out: &mut String) {
        let prefix = "  ".repeat(indent);
        let card = self.shape_cardinality(branch.syntax());
        let mark = Self::cardinality_mark(card);
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
            self.format_shape_expr(&body, indent + 1, out);
        }
    }

    fn cardinality_mark(card: ShapeCardinality) -> &'static str {
        match card {
            ShapeCardinality::One => "¹",
            ShapeCardinality::Many => "⁺",
        }
    }

    /// Format symbol references (without errors).
    pub fn format_refs(&self) -> String {
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

    fn format_tree(node: &SyntaxNode, indent: usize, out: &mut String, include_trivia: bool) {
        let prefix = "  ".repeat(indent);
        let _ = writeln!(out, "{}{:?}", prefix, node.kind());
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    Self::format_tree(&n, indent + 1, out, include_trivia)
                }
                rowan::NodeOrToken::Token(t) => {
                    if include_trivia || !t.kind().is_trivia() {
                        let child_prefix = "  ".repeat(indent + 1);
                        let _ = writeln!(out, "{}{:?} {:?}", child_prefix, t.kind(), t.text());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_query() {
        let q = Query::new("Expr = (expression)");
        assert!(q.is_valid());
        assert!(q.symbols().get("Expr").is_some());
    }

    #[test]
    fn parse_error() {
        let q = Query::new("(unclosed");
        assert!(!q.is_valid());
        assert!(q.render_errors().contains("expected"));
    }

    #[test]
    fn resolution_error() {
        let q = Query::new("(call (Undefined))");
        assert!(!q.is_valid());
        assert!(q.render_errors().contains("undefined reference"));
    }

    #[test]
    fn combined_errors() {
        let q = Query::new("(call (Undefined) extra)");
        assert!(!q.is_valid());
        // Both parse issues and resolution errors should be present
        assert!(!q.errors().is_empty());
    }

    #[test]
    fn error_stage_filtering() {
        use parser::ErrorStage;

        // Parse error only
        let q = Query::new("(unclosed");
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_by_stage(ErrorStage::Parse).len(), 1);

        // Resolve error only
        let q = Query::new("(call (Undefined))");
        assert!(!q.has_parse_errors());
        assert!(q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_by_stage(ErrorStage::Resolve).len(), 1);

        // Validate error only
        let q = Query::new("[A: (a) (b)]");
        assert!(!q.has_parse_errors());
        assert!(q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_by_stage(ErrorStage::Validate).len(), 1);

        // Escape error only
        let q = Query::new("Expr = (call (Expr))");
        assert!(!q.has_parse_errors());
        assert!(!q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
        assert_eq!(q.errors_by_stage(ErrorStage::Escape).len(), 1);

        // Mixed errors
        let q = Query::new("Expr = (call (Expr)) (unclosed");
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
    }

    #[test]
    fn render_errors_grouped() {
        let q = Query::new("Expr = (call (Expr)) (unclosed");
        let grouped = q.render_errors_grouped();
        assert!(grouped.contains("=== parse errors ==="));
        assert!(grouped.contains("=== escape errors ==="));
        assert!(!grouped.contains("=== resolve errors ==="));
    }
}
