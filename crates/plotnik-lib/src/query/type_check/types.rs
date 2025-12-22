//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Arity: Whether an expression matches one or many node positions (for field validation)
//! - TypeFlow: What data flows through an expression (for TypeScript emission)

use std::collections::BTreeMap;

/// Interned type identifier. Types are stored in TypeContext and referenced by ID.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(pub u32);

/// Void type - produces nothing, transparent
pub const TYPE_VOID: TypeId = TypeId(0);
/// Node type - a tree-sitter node
pub const TYPE_NODE: TypeId = TypeId(1);
/// String type - extracted text from a node via `:: string`
pub const TYPE_STRING: TypeId = TypeId(2);

impl TypeId {
    pub fn is_builtin(self) -> bool {
        self.0 <= 2
    }
}

/// The kind of a type, determining its structure.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeKind {
    /// Produces nothing, transparent to parent scope
    Void,
    /// A tree-sitter node
    Node,
    /// Extracted text from a node
    String,
    /// User-specified type via `@x :: TypeName`
    Custom(String),
    /// Object with named fields
    Struct(BTreeMap<String, FieldInfo>),
    /// Tagged union from labeled alternations
    Enum(BTreeMap<String, TypeId>),
    /// Array type with element type
    Array { element: TypeId, non_empty: bool },
    /// Optional wrapper
    Optional(TypeId),
    /// Forward reference to a recursive type
    Ref(String),
}

impl TypeKind {
    pub fn is_void(&self) -> bool {
        matches!(self, TypeKind::Void)
    }

    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            TypeKind::Node
                | TypeKind::String
                | TypeKind::Custom(_)
                | TypeKind::Struct(_)
                | TypeKind::Enum(_)
                | TypeKind::Array { .. }
                | TypeKind::Optional(_)
                | TypeKind::Ref(_)
        )
    }
}

/// Field information within a struct type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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

    pub fn fields(arity: Arity, fields: BTreeMap<String, FieldInfo>) -> Self {
        Self {
            arity,
            flow: TypeFlow::Fields(fields),
        }
    }
}

/// Structural arity - whether an expression matches one or many positions.
///
/// Used for field validation: `field: expr` requires `expr` to have `Arity::One`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Exactly one node position
    One,
    /// Multiple sequential positions
    Many,
}

impl Arity {
    /// Combine arities: Many wins
    pub fn combine(self, other: Arity) -> Arity {
        match (self, other) {
            (Arity::One, Arity::One) => Arity::One,
            _ => Arity::Many,
        }
    }
}

/// Data flow through an expression.
///
/// Determines what data an expression contributes to output:
/// - Void: Transparent, produces nothing (used for structural matching)
/// - Scalar: Opaque single value (captures, refs create scope boundaries)
/// - Fields: Transparent field contributions that bubble to parent
#[derive(Clone, Debug)]
pub enum TypeFlow {
    /// Transparent, produces nothing
    Void,
    /// Opaque single value that doesn't bubble
    Scalar(TypeId),
    /// Transparent fields that bubble to parent scope
    Fields(BTreeMap<String, FieldInfo>),
}

impl TypeFlow {
    pub fn is_void(&self) -> bool {
        matches!(self, TypeFlow::Void)
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, TypeFlow::Scalar(_))
    }

    pub fn is_fields(&self) -> bool {
        matches!(self, TypeFlow::Fields(_))
    }

    pub fn has_captures(&self) -> bool {
        match self {
            TypeFlow::Void => false,
            TypeFlow::Scalar(_) => false,
            TypeFlow::Fields(f) => !f.is_empty(),
        }
    }

    /// Get field names if this is a Fields flow
    pub fn field_names(&self) -> Option<Vec<&str>> {
        match self {
            TypeFlow::Fields(f) => Some(f.keys().map(|s| s.as_str()).collect()),
            _ => None,
        }
    }

    /// Create a single-field flow
    pub fn single_field(name: String, info: FieldInfo) -> Self {
        let mut fields = BTreeMap::new();
        fields.insert(name, info);
        TypeFlow::Fields(fields)
    }
}

/// Quantifier kind for type inference
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuantifierKind {
    /// `?` or `??` - zero or one, no dimensionality added
    Optional,
    /// `*` or `*?` - zero or more, adds dimensionality
    ZeroOrMore,
    /// `+` or `+?` - one or more, adds dimensionality
    OneOrMore,
}

impl QuantifierKind {
    /// Whether this quantifier requires strict dimensionality (row capture for internal captures)
    pub fn requires_row_capture(self) -> bool {
        matches!(self, QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore)
    }

    /// Whether the resulting array is non-empty
    pub fn is_non_empty(self) -> bool {
        matches!(self, QuantifierKind::OneOrMore)
    }
}
