//! Capture-mechanism classification now lives in `plotnik-compiler-core`;
//! re-exported here so the type-check pass and its consumers keep referring to it
//! as `crate::analyze::type_check::capture_mechanism::*`.

pub use plotnik_compiler_core::capture_mechanism::*;
