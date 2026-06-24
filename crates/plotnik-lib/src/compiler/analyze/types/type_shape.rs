//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Arity: Whether an expression matches one or many node positions.
//! - OutputFlow: What data flows through an expression.

use std::collections::BTreeMap;

pub use crate::compiler::ids::TypeId;

use crate::compiler::ids::DefId;
use crate::core::Symbol;

pub use crate::bytecode::type_system::Arity;
use crate::bytecode::type_system::PrimitiveType;
pub use crate::compiler::parse::ast::QuantifierKind;

pub const TYPE_VOID: TypeId = TypeId(PrimitiveType::Void.index() as u32);
pub const TYPE_NODE: TypeId = TypeId(PrimitiveType::Node.index() as u32);

/// The shape of an inferred type, determining its structure.
///
/// This represents the inference-time type representation which carries
/// actual data (fields, variants, inner types). Distinct from
/// `type_system::TypeKind` which is the bytecode format discriminant.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeShape {
    /// Produces nothing, transparent to parent scope.
    Void,
    /// A tree-sitter node.
    Node,
    /// User-specified type via `@x :: TypeName`.
    Custom(Symbol),
    /// Struct with named fields.
    Struct(BTreeMap<Symbol, FieldInfo>),
    /// Enum from an alternation with branch labels.
    Enum(BTreeMap<Symbol, TypeId>),
    /// Array type with element type.
    Array { element: TypeId, non_empty: bool },
    /// Optional wrapper.
    Optional(TypeId),
    /// Forward reference to a recursive type.
    Ref(DefId),
}

/// Field information within a struct type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FieldInfo {
    /// The type of this field's value.
    pub type_id: TypeId,
    /// Whether this field may be absent (from alternation branches).
    pub optional: bool,
}

impl FieldInfo {
    pub fn with_optional(type_id: TypeId, optional: bool) -> Self {
        Self { type_id, optional }
    }

    pub fn required(type_id: TypeId) -> Self {
        Self {
            type_id,
            optional: false,
        }
    }

    pub fn make_optional(self) -> Self {
        Self {
            optional: true,
            ..self
        }
    }
}

/// Data flow through an expression.
#[derive(Clone, Debug)]
pub enum OutputFlow {
    /// Transparent, produces nothing.
    Void,
    /// Opaque single value that doesn't bubble (scope boundary).
    Value(TypeId),
    /// Struct type whose fields bubble to parent scope.
    Fields(TypeId),
}

impl OutputFlow {
    pub fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    pub fn has_fields(&self) -> bool {
        matches!(self, Self::Fields(_))
    }

    pub fn type_id(&self) -> Option<TypeId> {
        match self {
            Self::Void => None,
            Self::Value(id) | Self::Fields(id) => Some(*id),
        }
    }
}

/// Combined arity and type flow information for an expression.
#[derive(Clone, Debug)]
pub struct PatternResult {
    /// How many times this expression matches (one vs many).
    pub arity: Arity,
    /// What data flows through this expression.
    pub flow: OutputFlow,
}

impl PatternResult {
    pub fn new(arity: Arity, flow: OutputFlow) -> Self {
        Self { arity, flow }
    }

    pub fn void() -> Self {
        Self {
            arity: Arity::One,
            flow: OutputFlow::Void,
        }
    }
}
