//! `Visitor`/`walk_*` now live in `plotnik-compiler-core`; re-exported here so the
//! analysis passes and their consumers keep referring to them as `crate::analyze::visitor::*`.

pub use plotnik_compiler_core::visitor::*;
