//! Result type definitions shared between analysis and bytecode.
//!
//! This module provides the canonical type model used across:
//! - Type checking/inference (`query::type_check`)
//! - Bytecode emission and runtime (`bytecode`)
//! - TypeScript code generation

mod kind;
mod primitives;

#[cfg(test)]
mod kind_tests;
#[cfg(test)]
mod primitives_tests;

pub use kind::TypeKind;
pub use primitives::{PrimitiveType, TYPE_BOOL, TYPE_CUSTOM_START, TYPE_NODE, TYPE_STR, TYPE_VOID};
