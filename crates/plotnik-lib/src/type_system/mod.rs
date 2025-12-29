//! Core type system definitions shared between analysis and bytecode.
//!
//! This module provides the canonical type model used across:
//! - Type checking/inference (`query::type_check`)
//! - Bytecode emission and runtime (`bytecode`)
//! - TypeScript code generation

mod arity;
mod kind;
mod primitives;
mod quantifier;

pub use arity::Arity;
pub use kind::TypeKind;
pub use primitives::{PrimitiveType, TYPE_CUSTOM_START, TYPE_NODE, TYPE_STRING, TYPE_VOID};
pub use quantifier::QuantifierKind;
