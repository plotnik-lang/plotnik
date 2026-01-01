//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Arity: Whether an expression matches one or many node positions.
//! - TypeFlow: What data flows through an expression.

use std::collections::BTreeMap;

use super::symbol::{DefId, Symbol};

// Re-export shared type system components
pub use crate::type_system::{Arity, QuantifierKind};
use crate::type_system::{PrimitiveType, TYPE_STRING as PRIM_TYPE_STRING};

/// Interned type identifier.
///
/// Index into the type registry. Values 0-2 are reserved for builtins
/// (Void, Node, String); custom types start at index 3.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(pub u32);

pub const TYPE_VOID: TypeId = TypeId(PrimitiveType::Void.index() as u32);
pub const TYPE_NODE: TypeId = TypeId(PrimitiveType::Node.index() as u32);
pub const TYPE_STRING: TypeId = TypeId(PrimitiveType::String.index() as u32);

impl TypeId {
    pub fn is_builtin(self) -> bool {
        self.0 <= PRIM_TYPE_STRING as u32
    }
}

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
    /// Extracted text from a node.
    String,
    /// User-specified type via `@x :: TypeName`.
    Custom(Symbol),
    /// Object with named fields.
    Struct(BTreeMap<Symbol, FieldInfo>),
    /// Tagged union from labeled alternations.
    Enum(BTreeMap<Symbol, TypeId>),
    /// Array type with element type.
    Array { element: TypeId, non_empty: bool },
    /// Optional wrapper.
    Optional(TypeId),
    /// Forward reference to a recursive type.
    Ref(DefId),
}

impl TypeShape {
    pub fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    pub fn is_scalar(&self) -> bool {
        !self.is_void()
    }
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
    pub fn required(type_id: TypeId) -> Self {
        Self {
            type_id,
            optional: false,
        }
    }

    pub fn optional(type_id: TypeId) -> Self {
        Self {
            type_id,
            optional: true,
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
pub enum TypeFlow {
    /// Transparent, produces nothing.
    Void,
    /// Opaque single value that doesn't bubble (scope boundary).
    Scalar(TypeId),
    /// Struct type whose fields bubble to parent scope.
    Bubble(TypeId),
}

impl TypeFlow {
    pub fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }

    pub fn is_bubble(&self) -> bool {
        matches!(self, Self::Bubble(_))
    }

    pub fn type_id(&self) -> Option<TypeId> {
        match self {
            Self::Void => None,
            Self::Scalar(id) | Self::Bubble(id) => Some(*id),
        }
    }
}

/// Combined arity and type flow information for an expression.
#[derive(Clone, Debug)]
pub struct TermInfo {
    /// How many times this expression matches (one vs many).
    pub arity: Arity,
    /// What data flows through this expression.
    pub flow: TypeFlow,
}

impl TermInfo {
    pub fn new(arity: Arity, flow: TypeFlow) -> Self {
        Self { arity, flow }
    }

    pub fn void() -> Self {
        Self {
            arity: Arity::One,
            flow: TypeFlow::Void,
        }
    }

    pub fn node() -> Self {
        Self {
            arity: Arity::One,
            flow: TypeFlow::Void,
        }
    }

    pub fn scalar(arity: Arity, type_id: TypeId) -> Self {
        Self {
            arity,
            flow: TypeFlow::Scalar(type_id),
        }
    }

    pub fn bubble(arity: Arity, struct_type_id: TypeId) -> Self {
        Self {
            arity,
            flow: TypeFlow::Bubble(struct_type_id),
        }
    }
}
