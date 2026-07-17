//! Core type definitions for the type checking pass.
//!
//! The type system tracks two orthogonal properties:
//! - Root extent: whether a match has exactly one top-level node.
//! - Pattern flow: what result data flows through an expression.

use std::collections::BTreeMap;

pub use crate::compiler::ids::TypeId;

use crate::compiler::ids::TypeDeclId;
use crate::core::Symbol;

use super::RootExtent;
use super::capture::InferredFieldFlow;
use crate::bytecode::type_system::PrimitiveType;
pub use crate::compiler::parse::ast::QuantifierKind;

pub(crate) const RESERVED_NO_VALUE_TYPE_ID: TypeId = TypeId(PrimitiveType::NoValue.index() as u32);
pub const TYPE_NODE: TypeId = TypeId(PrimitiveType::Node.index() as u32);
pub const TYPE_TEXT: TypeId = TypeId(PrimitiveType::Text.index() as u32);
pub const TYPE_BOOL: TypeId = TypeId(PrimitiveType::Bool.index() as u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ListMinimum {
    Zero,
    One,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DefinitionOutput {
    MatchOnly,
    Value(TypeId),
}

impl DefinitionOutput {
    pub fn value(self) -> Option<TypeId> {
        match self {
            Self::MatchOnly => None,
            Self::Value(type_id) => Some(type_id),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CasePayload {
    NoPayload,
    Record(TypeId),
}

impl CasePayload {
    pub fn type_id(self) -> Option<TypeId> {
        match self {
            Self::NoPayload => None,
            Self::Record(type_id) => Some(type_id),
        }
    }
}

/// The shape of an inferred type, determining its structure.
///
/// This represents the inference-time type representation which carries
/// actual data (fields, cases, inner types). Distinct from
/// `type_system::TypeKind`, the bytecode discriminant.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeShape {
    /// A tree-sitter node.
    Node,
    /// Borrowed source text.
    Text,
    /// Boolean value.
    Bool,
    /// Record with named fields.
    Record(BTreeMap<Symbol, RecordField>),
    /// Variant type from a labeled alternation.
    Variant(BTreeMap<Symbol, CasePayload>),
    /// Ordered list with its semantic minimum length.
    List {
        element: TypeId,
        minimum: ListMinimum,
    },
    /// Option type containing zero or one value.
    Option(TypeId),
    /// Reference to a named type declaration.
    Ref(TypeDeclId),
}

type RecordFieldTypeIds<'a> = std::iter::Map<
    std::collections::btree_map::Values<'a, Symbol, RecordField>,
    fn(&RecordField) -> TypeId,
>;
type CasePayloadTypeIds<'a> = std::iter::FilterMap<
    std::iter::Copied<std::collections::btree_map::Values<'a, Symbol, CasePayload>>,
    fn(CasePayload) -> Option<TypeId>,
>;

pub struct TypeShapeChildIds<'a>(TypeShapeChildIdsInner<'a>);

enum TypeShapeChildIdsInner<'a> {
    Fields(RecordFieldTypeIds<'a>),
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
            Self::Record(fields) => TypeShapeChildIdsInner::Fields(
                fields
                    .values()
                    .map(field_type_id as fn(&RecordField) -> TypeId),
            ),
            Self::Variant(cases) => TypeShapeChildIdsInner::Cases(
                cases
                    .values()
                    .copied()
                    .filter_map(CasePayload::type_id as fn(CasePayload) -> Option<TypeId>),
            ),
            Self::List { element, .. } | Self::Option(element) => {
                TypeShapeChildIdsInner::One(Some(*element).into_iter())
            }
            Self::Node | Self::Text | Self::Bool | Self::Ref(_) => {
                TypeShapeChildIdsInner::Empty(std::iter::empty())
            }
        };
        TypeShapeChildIds(inner)
    }
}

fn field_type_id(field: &RecordField) -> TypeId {
    field.final_type
}

/// One finalized field in a record type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RecordField {
    pub final_type: TypeId,
}

impl RecordField {
    pub fn new(final_type: TypeId) -> Self {
        Self { final_type }
    }
}

/// Data flow through an expression.
#[derive(Clone, Debug)]
pub enum PatternFlow {
    /// Transparent, produces no value.
    NoValue,
    /// Opaque single value that doesn't bubble (scope boundary).
    Value(TypeId),
    /// Record field set that bubbles to the parent scope.
    Fields(TypeId),
}

impl PatternFlow {
    pub fn is_no_value(&self) -> bool {
        matches!(self, Self::NoValue)
    }

    pub fn has_fields(&self) -> bool {
        matches!(self, Self::Fields(_))
    }

    pub fn type_id(&self) -> Option<TypeId> {
        match self {
            Self::NoValue => None,
            Self::Value(id) | Self::Fields(id) => Some(*id),
        }
    }
}

/// Combined root extent and result flow for a pattern.
#[derive(Clone, Debug)]
pub struct PatternShape {
    /// Whether one match has exactly one top-level syntax-tree node.
    pub root_extent: RootExtent,
    /// What data flows through this expression.
    pub flow: PatternFlow,
    pub(super) field_flow: Option<InferredFieldFlow>,
}

impl PatternShape {
    pub fn new(root_extent: RootExtent, flow: PatternFlow) -> Self {
        Self {
            root_extent,
            flow,
            field_flow: None,
        }
    }

    pub(super) fn fields(root_extent: RootExtent, field_flow: InferredFieldFlow) -> Self {
        Self {
            root_extent,
            flow: PatternFlow::Fields(field_flow.type_id),
            field_flow: Some(field_flow),
        }
    }

    pub fn no_value() -> Self {
        Self {
            root_extent: RootExtent::SingleNode,
            flow: PatternFlow::NoValue,
            field_flow: None,
        }
    }
}
