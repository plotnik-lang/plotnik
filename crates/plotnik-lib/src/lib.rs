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

use ql::ast::Root;
use ql::parser::{self, Parse, SyntaxError};
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
        parser::render_errors(&self.source, &self.errors)
    }
}

#[cfg(test)]
impl Query<'_> {
    /// Snapshot of AST structure (without trivia).
    pub fn snapshot_ast(&self) -> String {
        let mut out = String::new();
        Self::format_tree(&self.syntax(), 0, &mut out, false);
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
            out.push('\n');
        }
        out
    }

    /// Snapshot of AST structure (with trivia).
    pub fn snapshot_ast_raw(&self) -> String {
        let mut out = String::new();
        Self::format_tree(&self.syntax(), 0, &mut out, true);
        if !self.errors.is_empty() {
            out.push_str("---\n");
            out.push_str(&self.render_errors());
            out.push('\n');
        }
        out
    }

    /// Snapshot of symbol references.
    pub fn snapshot_refs(&self) -> String {
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
                        let _ = writeln!(out, "{}  {:?} {:?}", prefix, t.kind(), t.text());
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
}
