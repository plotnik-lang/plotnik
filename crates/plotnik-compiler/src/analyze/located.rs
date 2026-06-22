//! `Located<T>` now lives in `plotnik-compiler-core`; re-exported here so the
//! analysis passes and their consumers keep referring to it as `crate::analyze::Located`.

pub use plotnik_compiler_core::Located;
