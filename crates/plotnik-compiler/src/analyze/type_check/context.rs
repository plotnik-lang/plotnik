//! `TypeContext` now lives in `plotnik-compiler-core`; re-exported here so the
//! type-check pass and its consumers keep referring to it as
//! `crate::analyze::type_check::TypeContext`.

pub use plotnik_compiler_core::TypeContext;
