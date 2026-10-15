//! Proc-macro shell for Plotnik: `query!` runs the full compiler pipeline at
//! build time and expands to the generated query module (typed result types,
//! supplied-node execution methods, and the compiled matcher).
//!
//! Use it through the `plotnik` facade crate. Generated code uses
//! `::plotnik::rt::...` runtime paths.

use proc_macro::TokenStream;

mod args;
mod expand;
mod grammar_source;

/// Compile a Plotnik query into a typed Rust module at the invocation site.
///
/// ```ignore
/// plotnik::query! {
///     r#"
///     Q = (program (expression_statement (identifier) @id))
///     "#,
///     grammar = "tree-sitter-javascript",
/// }
/// ```
///
/// Arguments (comma-separated, any order; the query is the one bare string
/// literal):
///
/// - `grammar = "..."` (required) — where the grammar comes from:
///   - a package name from your dependency graph (`"tree-sitter-javascript"`,
///     any crate shipping a `grammar.json`),
///   - `"package/subgrammar"` when one package ships several grammars
///     (`"tree-sitter-typescript/tsx"`),
///   - a `grammar.json` path (`"./grammars/mylang.json"`, resolved like
///     `include_str!` — relative to the invoking file).
/// - `file = "queries/q.ptk"` — read the query from a file (relative to the
///   invoking file) instead of an inline literal.
/// - `fuel = <n> | auto | unbounded`, `memory = <n> | auto | unbounded`,
///   `depth = <n> | auto` — the limit policy compiled into the safe entry
///   points (default `auto`). `fuel` bounds cumulative traversal and matcher work,
///   `memory` samples live matcher state, `depth` bounds the committed
///   value's nesting (typed decoding recurses once per nested value, so
///   this is its native-stack guard).
///
/// The expansion is item-position only (it defines types); invoke it at
/// module scope, not inside a function body.
/// Eligible entries expose `exec(node, source)`, `each(scope, source)`,
/// `find(scope, source)`, and `find_all(scope, source)`. Match-only entries
/// return the matched node. Data-producing entries return their generated type.
#[proc_macro]
pub fn query(input: TokenStream) -> TokenStream {
    expand::expand(input)
}
