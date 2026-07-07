//! Plotnik: typed tree-sitter queries.
//!
//! This crate is the developer entry point: [`query!`](query) compiles a
//! Plotnik query at build time into typed output structs/enums with
//! `parse`/`matches` entry points — no bytecode, no dynamic values.
//!
//! ```ignore
//! // `query!` defines types, so invoke it at module scope — not inside a function.
//! plotnik::query! {
//!     r#"
//!     Q = (program (expression_statement (identifier) @id))
//!     "#,
//!     grammar = "tree-sitter-javascript",
//! }
//!
//! fn main() {
//!     let language = tree_sitter_javascript::LANGUAGE.into();
//!     let mut parser = plotnik::tree_sitter::Parser::new();
//!     parser.set_language(&language).unwrap();
//!     let source = "x;";
//!     let tree = parser.parse(source, None).unwrap();
//!     // `parse` is the trusted-input path; use `Q::try_parse` for untrusted source.
//!     let q = Q::parse(&tree, source).expect("matches");
//! }
//! ```
//!
//! There is no built-in language list: `grammar = "..."` names any package in
//! your own dependency graph that ships a `grammar.json` (`tree-sitter-*`,
//! `arborium-*`, or your own grammar crate), so the baked grammar is exactly
//! the version your lockfile resolves — the same package whose parser you
//! link at runtime. The generated module double-checks every tree it is
//! handed against that grammar and panics on version skew.
//!
//! For the dynamic side of Plotnik (running query files against sources,
//! inspecting ASTs, tracing execution), use the `plotnik-cli` crate.

/// Compile a Plotnik query to a typed Rust module at build time.
///
/// See the crate docs for an example; the macro's own documentation lists
/// every argument. Generated code reaches the runtime through
/// [`::plotnik::rt`](rt), which is why this facade is the intended way in.
pub use plotnik_macros::query;

/// The shared runtime engine generated query modules run on. Generated code
/// spells every runtime path as `::plotnik::rt::...`.
pub use plotnik_rt as rt;

/// The tree-sitter runtime this crate is built against, re-exported so
/// callers parse sources with the exact version the engine links.
pub use tree_sitter;
