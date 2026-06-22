//! The type-system data model now lives in `plotnik-compiler-core`; re-exported here so
//! the type-check pass and its consumers keep referring to it as
//! `crate::analyze::type_check::types::*`.

pub use plotnik_compiler_core::type_shape::*;
