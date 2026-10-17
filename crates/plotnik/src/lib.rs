//! Plotnik: typed tree-sitter queries.
//!
//! This crate is the developer entry point: [`query!`](query) compiles a
//! Plotnik query at build time into typed output structs/enums with
//! `exec`/`each`/`find`/`find_all` methods, with no bytecode or dynamic values.
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
//!     let q = Q::exec(tree.root_node(), source)
//!         .expect("auto limits fit")
//!         .expect("matches");
//!     assert_eq!(q.view().id.utf8_text(source.as_bytes()).unwrap(), "x");
//! }
//! ```
//!
//! Value-producing queries return an owning `rt::Match`. Read the generated
//! row through `view()`. Rows are shallow `Copy` values: lists are borrowed
//! slices and recursive values are references into the match's storage.
//! Each item from `each` owns its storage independently. `find_all` returns
//! `rt::Matches`, which supports borrowing iteration with `for row in &matches`.
//! Node fields borrow the parse tree, text fields borrow the source, and
//! slices and recursive references cannot outlive their result owner.
//! Use `Match::write_json` to render a result without recursive serialization.
//! Match-only queries return the matched node instead of a result owner.
//!
//! There is no built-in language list: `grammar = "..."` names any package in
//! your own dependency graph that ships a `grammar.json` (`tree-sitter-*` or
//! your own grammar crate), so the baked grammar is exactly
//! the version your lockfile resolves — the same package whose parser you
//! link at runtime. The generated module double-checks every tree it is
//! handed against that grammar and panics on version skew.
//!
//! Enable `wasm` to compile and run queries at runtime through `dynamic`.
//! Supply grammar metadata and parser bytes to `WasmParser`.

/// Compile a Plotnik query to a typed Rust module at build time.
///
/// See the crate docs for an example; the macro's own documentation lists
/// every argument. Generated code reaches the runtime through
/// [`::plotnik::rt`](rt), which is why this facade is the intended way in.
pub use plotnik_macros::query;

pub use plotnik_runtime::{Execute, LimitExceeded, each, exec, find, find_all};

/// The shared runtime engine generated query modules run on. Generated code
/// spells every runtime path as `::plotnik::rt::...`.
pub use plotnik_runtime as rt;

/// The tree-sitter runtime this crate is built against, re-exported so
/// callers parse sources with the exact version the engine links.
pub use plotnik_runtime::tree_sitter;

/// Query compilation and execution.
#[cfg(feature = "wasm")]
pub use plotnik_compiler as dynamic;
#[cfg(feature = "wasm")]
pub use plotnik_compiler::{WasmParser, WasmParserError};
