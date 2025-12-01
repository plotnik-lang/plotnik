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

pub mod ql;

use ql::ast::{Root, format_ast};
use ql::parser::{self, ErrorStage, Parse, SyntaxError};
use ql::resolve::SymbolTable;
use ql::syntax_kind::SyntaxNode;

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
}

impl<'a> Query<'a> {
    /// Parse and resolve a query from source text.
    ///
    /// This never fails. Parse and resolution errors are collected
    /// and accessible via [`errors`](Self::errors).
    pub fn new(source: &'a str) -> Self {
        let parse = parser::parse(source);

        let root = Root::cast(parse.syntax()).expect("parser always produces Root");
        let resolve_result = ql::resolve::resolve(&root);

        let mut errors = parse.errors().to_vec();
        errors.extend(resolve_result.errors);

        // Check for recursive patterns with no escape path
        let escape_errors = ql::escape::check_escape(&root, &resolve_result.symbols);
        errors.extend(escape_errors);

        Self {
            source,
            parse,
            symbols: resolve_result.symbols,
            errors,
        }
    }

    /// The original source text.
    pub fn source(&self) -> &str {
        &self.source
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
        parser::render_errors(&self.source, &self.errors, None)
    }

    /// Filter errors by stage.
    pub fn errors_by_stage(&self, stage: ErrorStage) -> Vec<&SyntaxError> {
        self.errors.iter().filter(|e| e.stage == stage).collect()
    }

    /// Returns `true` if there are parse-stage errors.
    pub fn has_parse_errors(&self) -> bool {
        self.errors.iter().any(|e| e.stage == ErrorStage::Parse)
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
        parser::render_errors(&self.source, &filtered, None)
    }

    /// Render errors grouped by stage.
    pub fn render_errors_grouped(&self) -> String {
        let mut out = String::new();
        for stage in [ErrorStage::Parse, ErrorStage::Resolve, ErrorStage::Escape] {
            let stage_errors: Vec<_> = self.errors_by_stage(stage).into_iter().cloned().collect();
            if !stage_errors.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("=== {} errors ===\n", stage));
                out.push_str(&parser::render_errors(&self.source, &stage_errors, None));
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

    /// Snapshot of CST structure (without trivia, with errors).
    pub fn snapshot_cst(&self) -> String {
        let mut out = self.format_cst();
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
            out.push('\n');
        }
        out
    }

    /// Snapshot of AST structure (with errors).
    pub fn snapshot_ast(&self) -> String {
        let mut out = self.format_ast();
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
            out.push('\n');
        }
        out
    }

    /// Snapshot of CST structure (with trivia, with errors).
    pub fn snapshot_cst_raw(&self) -> String {
        let mut out = self.format_cst_raw();
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
            out.push('\n');
        }
        out
    }

    /// Snapshot of symbol references (with errors).
    pub fn snapshot_refs(&self) -> String {
        let mut out = self.format_refs();
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
        }
        out
    }

    fn format_tree(node: &SyntaxNode, indent: usize, out: &mut String, include_trivia: bool) {
        use std::fmt::Write;
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
        use ql::parser::ErrorStage;

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

        // Escape error only
        let q = Query::new("Expr = (call (Expr))");
        assert!(!q.has_parse_errors());
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
