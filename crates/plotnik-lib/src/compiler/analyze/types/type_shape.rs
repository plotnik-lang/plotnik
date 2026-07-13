//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Arity: Whether an expression matches one or many node positions.
//! - PatternFlow: What data flows through an expression.

use std::collections::BTreeMap;

pub use crate::compiler::ids::TypeId;

use crate::compiler::ids::DefId;
use crate::core::Symbol;

pub use crate::bytecode::type_system::Arity;
use crate::bytecode::type_system::PrimitiveType;
pub use crate::compiler::parse::ast::QuantifierKind;

pub const TYPE_VOID: TypeId = TypeId(PrimitiveType::Void.index() as u32);
pub const TYPE_NODE: TypeId = TypeId(PrimitiveType::Node.index() as u32);
pub const TYPE_STR: TypeId = TypeId(PrimitiveType::Str.index() as u32);
pub const TYPE_BOOL: TypeId = TypeId(PrimitiveType::Bool.index() as u32);

/// The shape of an inferred type, determining its structure.
///
/// This represents the inference-time type representation which carries
/// actual data (fields, cases, inner types). Distinct from
/// `type_system::TypeKind`, the bytecode discriminant.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeShape {
    /// Produces nothing, transparent to parent scope.
    Void,
    /// A tree-sitter node.
    Node,
    /// Borrowed source text.
    Str,
    /// Boolean value.
    Bool,
    /// User-specified name for a captured node via `@x :: TypeName`.
    Custom(Symbol),
    /// Struct with named fields.
    Struct(BTreeMap<Symbol, FieldInfo>),
    /// Variant type from a labeled alternation.
    Variant(BTreeMap<Symbol, TypeId>),
    /// Array type with element type.
    Array { element: TypeId, non_empty: bool },
    /// Optional wrapper.
    Optional(TypeId),
    /// Forward reference to a recursive type.
    Ref(DefId),
}

type FieldTypeIds<'a> = std::iter::Map<
    std::collections::btree_map::Values<'a, Symbol, FieldInfo>,
    fn(&FieldInfo) -> TypeId,
>;
type CasePayloadTypeIds<'a> =
    std::iter::Copied<std::collections::btree_map::Values<'a, Symbol, TypeId>>;

pub struct TypeShapeChildIds<'a>(TypeShapeChildIdsInner<'a>);

enum TypeShapeChildIdsInner<'a> {
    Fields(FieldTypeIds<'a>),
    Cases(CasePayloadTypeIds<'a>),
    One(std::option::IntoIter<TypeId>),
    Empty(std::iter::Empty<TypeId>),
}

impl Iterator for TypeShapeChildIds<'_> {
    type Item = TypeId;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            TypeShapeChildIdsInner::Fields(ids) => ids.next(),
            TypeShapeChildIdsInner::Cases(ids) => ids.next(),
            TypeShapeChildIdsInner::One(id) => id.next(),
            TypeShapeChildIdsInner::Empty(ids) => ids.next(),
        }
    }
}

impl TypeShape {
    pub fn child_type_ids(&self) -> TypeShapeChildIds<'_> {
        let inner = match self {
            Self::Struct(fields) => TypeShapeChildIdsInner::Fields(
                fields
                    .values()
                    .map(field_type_id as fn(&FieldInfo) -> TypeId),
            ),
            Self::Variant(cases) => TypeShapeChildIdsInner::Cases(cases.values().copied()),
            Self::Array { element, .. } | Self::Optional(element) => {
                TypeShapeChildIdsInner::One(Some(*element).into_iter())
            }
            Self::Void | Self::Node | Self::Str | Self::Bool | Self::Custom(_) | Self::Ref(_) => {
                TypeShapeChildIdsInner::Empty(std::iter::empty())
            }
        };
        TypeShapeChildIds(inner)
    }
}

fn field_type_id(field: &FieldInfo) -> TypeId {
    field.type_id
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
pub enum PatternFlow {
    /// Transparent, produces nothing.
    Void,
    /// Opaque single value that doesn't bubble (scope boundary).
    Value(TypeId),
    /// Struct type whose fields bubble to parent scope.
    Fields(TypeId),
}

impl PatternFlow {
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
pub struct PatternShape {
    /// How many times this expression matches (one vs many).
    pub arity: Arity,
    /// What data flows through this expression.
    pub flow: PatternFlow,
}

impl PatternShape {
    pub fn new(arity: Arity, flow: PatternFlow) -> Self {
        Self { arity, flow }
    }

    pub fn void() -> Self {
        Self {
            arity: Arity::One,
            flow: PatternFlow::Void,
        }
    }
}
