//! Proc-macro shell for Plotnik: `query!` runs the full compiler pipeline at
//! build time and expands to the generated query module (typed output types,
//! `parse`/`matches` entry points, and the compiled matcher).
//!
//! All compilation logic lives in `plotnik-lib` (built compiler-only, no C);
//! this crate only parses macro arguments, locates the grammar, and splices
//! the generated module into the caller. Use it through the `plotnik` facade
//! crate — generated code spells its runtime paths as `::plotnik::rt::...`
//! unless re-pointed with the `crate` argument.

use proc_macro::TokenStream;

mod args;
mod expand;
mod grammar_source;

#[cfg(test)]
mod grammar_source_tests;

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
///     `"arborium-javascript"`, any crate shipping a `grammar.json`),
///   - `"package/subgrammar"` when one package ships several grammars
///     (`"tree-sitter-typescript/tsx"`),
///   - a `grammar.json` path (`"./grammars/mylang.json"`, resolved like
///     `include_str!` — relative to the invoking file).
/// - `file = "queries/q.ptk"` — read the query from a file (relative to the
///   invoking file) instead of an inline literal.
/// - `crate = ::path::to::rt` — respell the runtime-crate path baked into
///   generated code; defaults to `::plotnik::rt`.
/// - `steps = <n> | auto | unbounded`, `memory = <n> | auto | unbounded`,
///   `depth = <n> | auto` — the limit policy compiled into the safe entry
///   points (default `auto`). `steps` bound total work,
///   `memory` bounds live backtracking state, `depth` bounds the committed
///   value's nesting (the typed replay recurses once per nested value, so
///   this is its native-stack guard).
///
/// The expansion is item-position only (it defines types); invoke it at
/// module scope, not inside a function body.
#[proc_macro]
pub fn query(input: TokenStream) -> TokenStream {
    expand::expand(input)
}
