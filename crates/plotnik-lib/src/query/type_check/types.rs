//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Arity: Whether an expression matches one or many node positions.
//! - TypeFlow: What data flows through an expression.

use std::collections::BTreeMap;

use super::symbol::{DefId, Symbol};

/// Interned type identifier.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(pub u32);

pub const TYPE_VOID: TypeId = TypeId(0);
pub const TYPE_NODE: TypeId = TypeId(1);
pub const TYPE_STRING: TypeId = TypeId(2);

impl TypeId {
    pub fn is_builtin(self) -> bool {
        self.0 <= TYPE_STRING.0
    }
}

/// The kind of a type, determining its structure.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeKind {
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

impl TypeKind {
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
    pub type_id: TypeId,
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

/// Structural arity - whether an expression matches one or many positions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exactly one node position.
    One,
    /// Multiple sequential positions.
    Many,
}

impl Arity {
    /// Combine arities: Many wins.
    pub fn combine(self, other: Self) -> Self {
        if self == Self::One && other == Self::One {
            return Self::One;
        }
        Self::Many
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
    pub arity: Arity,
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

/// Quantifier kind for type inference.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuantifierKind {
    /// `?` or `??` - zero or one.
    Optional,
    /// `*` or `*?` - zero or more.
    ZeroOrMore,
    /// `+` or `+?` - one or more.
    OneOrMore,
}

impl QuantifierKind {
    /// Whether this quantifier requires strict dimensionality (row capture).
    pub fn requires_row_capture(self) -> bool {
        matches!(self, Self::ZeroOrMore | Self::OneOrMore)
    }

    pub fn is_non_empty(self) -> bool {
        matches!(self, Self::OneOrMore)
    }
}
