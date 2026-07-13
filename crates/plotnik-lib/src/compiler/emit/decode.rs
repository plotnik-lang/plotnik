//! Target-neutral typed result-decoding plan.
//!
//! Match-journal events name record fields and variant cases with absolute layout
//! slots. This plan resolves every nominal twin to the complete set of slots
//! it may produce and turns output types into a small value-decoding algebra.
//! Backends choose identifiers, error syntax, recursion representation, and
//! construction syntax without walking analysis types or rebuilding tables.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::compiler::analyze::output::{CaptureLayout, OutputItem, OutputItemKind, OutputSchema};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    DefinitionOutput, RecordField, TypeId, TypeShape,
};
use crate::core::Symbol;

#[derive(Clone, Debug)]
pub(crate) struct ResultDecodePlan {
    items: Vec<DecodeItem>,
    items_by_name: HashMap<Symbol, usize>,
}

impl ResultDecodePlan {
    pub(super) fn build(schema: &OutputSchema<'_>) -> Self {
        let builder = DecodePlanBuilder { schema };
        let items = schema
            .entry_point_items()
            .iter()
            .map(|item| builder.item(*item))
            .collect::<Vec<_>>();
        let items_by_name = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.name, index))
            .collect();
        Self {
            items,
            items_by_name,
        }
    }

    pub(crate) fn items(&self) -> &[DecodeItem] {
        &self.items
    }

    pub(crate) fn item(&self, name: Symbol) -> &DecodeItem {
        let index = self
            .items_by_name
            .get(&name)
            .expect("every decode target declares an output item");
        &self.items[*index]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeItem {
    pub(crate) name: Symbol,
    pub(crate) output: DefinitionOutput,
    pub(crate) kind: DecodeItemKind,
    /// This decoder is a recursive definition's dynamic depth boundary.
    pub(crate) enters_depth: bool,
    /// This decoder or one it calls can trip the decode-depth limit.
    pub(crate) fallible: bool,
}

impl DecodeItem {
    pub(crate) fn has_decoder(&self) -> bool {
        !matches!(self.kind, DecodeItemKind::MatchOnlyDefinition)
    }

    pub(crate) fn value_type(&self) -> TypeId {
        self.output
            .value()
            .expect("value decoder must have a value type")
    }
}

#[derive(Clone, Debug)]
pub(crate) enum DecodeItemKind {
    Record(DecodeScope),
    Variant(Vec<DecodeCase>),
    Alias(DecodeValue),
    MatchOnlyDefinition,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeScope {
    pub(crate) fields: Vec<DecodeField>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeField {
    pub(crate) name: Symbol,
    /// Every absolute `RecordSet` slot this field may use across nominal twins.
    pub(crate) indices: Vec<u16>,
    pub(crate) value: DecodeValue,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeCase {
    pub(crate) name: Symbol,
    /// Every absolute `VariantOpen` slot this case may use across twins.
    pub(crate) indices: Vec<u16>,
    pub(crate) payload: Option<DecodeScope>,
}

#[derive(Clone, Debug)]
pub(crate) enum DecodeValue {
    Node,
    Text,
    Bool,
    Nullable(Box<DecodeValue>),
    List(Box<DecodeValue>),
    Nested {
        item: Symbol,
        /// Analysis type at this read position. It preserves whether the
        /// position is a direct nominal value or a definition-reference
        /// occurrence without prescribing a target's representation.
        source_type: TypeId,
    },
}

struct DecodePlanBuilder<'p, 'a> {
    schema: &'p OutputSchema<'a>,
}

impl DecodePlanBuilder<'_, '_> {
    fn item(&self, item: OutputItem) -> DecodeItem {
        let kind = match item.kind {
            OutputItemKind::Record => {
                let TypeShape::Record(fields) =
                    self.schema.types.expect_type_shape(item.value_type())
                else {
                    unreachable!("record output item has a record shape");
                };
                let twins = collect_twins(self.schema.types, self.schema.layout(), item);
                DecodeItemKind::Record(self.scope(fields.iter(), &twins))
            }
            OutputItemKind::Variant => DecodeItemKind::Variant(self.variant_cases(item)),
            OutputItemKind::Alias => DecodeItemKind::Alias(self.value(item.value_type())),
            OutputItemKind::MatchOnlyDef => DecodeItemKind::MatchOnlyDefinition,
        };
        DecodeItem {
            name: item.name,
            output: item.output,
            kind,
            enters_depth: self.item_enters_depth(item.name),
            fallible: self.item_is_fallible(item),
        }
    }

    fn variant_cases(&self, item: OutputItem) -> Vec<DecodeCase> {
        let TypeShape::Variant(cases) = self.schema.types.expect_type_shape(item.value_type())
        else {
            unreachable!("variant output item has a variant shape");
        };
        let twins = collect_twins(self.schema.types, self.schema.layout(), item);
        cases
            .iter()
            .enumerate()
            .map(|(index, (&name, &payload))| {
                let payload = payload.type_id().map(|payload| {
                    let TypeShape::Record(fields) = self.schema.types.expect_type_shape(payload)
                    else {
                        unreachable!("variant case has no payload or an anonymous record payload");
                    };
                    let payloads = payload_twins(self.schema.types, &twins, index);
                    self.scope(fields.iter(), &payloads)
                });
                DecodeCase {
                    name,
                    indices: member_indices(self.schema.layout(), &twins, index),
                    payload,
                }
            })
            .collect()
    }

    fn scope<'b>(
        &self,
        fields: impl Iterator<Item = (&'b Symbol, &'b RecordField)>,
        twins: &[TypeId],
    ) -> DecodeScope {
        DecodeScope {
            fields: fields
                .enumerate()
                .map(|(index, (&name, info))| DecodeField {
                    name,
                    indices: member_indices(self.schema.layout(), twins, index),
                    value: self.value(info.final_type),
                })
                .collect(),
        }
    }

    fn value(&self, ty: TypeId) -> DecodeValue {
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node | TypeShape::Custom(_) => DecodeValue::Node,
            TypeShape::Text => DecodeValue::Text,
            TypeShape::Bool => DecodeValue::Bool,
            TypeShape::Option(inner) => DecodeValue::Nullable(Box::new(self.value(*inner))),
            TypeShape::List { element, .. } => DecodeValue::List(Box::new(self.value(*element))),
            TypeShape::Record(_) | TypeShape::Variant(_) => DecodeValue::Nested {
                item: self
                    .schema
                    .type_name_of(ty)
                    .expect("naming pass names every non-payload composite"),
                source_type: ty,
            },
            TypeShape::Ref(definition) => {
                if self
                    .schema
                    .types
                    .expect_def_output(*definition)
                    .value()
                    .is_none()
                {
                    return DecodeValue::Node;
                }
                DecodeValue::Nested {
                    item: self.schema.deps.def_name_sym(*definition),
                    source_type: ty,
                }
            }
        }
    }

    fn item_enters_depth(&self, name: Symbol) -> bool {
        self.schema
            .deps
            .def_id_for_sym(name)
            .is_some_and(|definition| self.schema.deps.is_recursive_def(definition))
    }

    fn item_is_fallible(&self, item: OutputItem) -> bool {
        self.item_enters_depth(item.name)
            || item.output.value().is_some_and(|type_id| {
                self.type_reaches_recursive_ref(type_id, &mut HashSet::new())
            })
    }

    fn type_reaches_recursive_ref(&self, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        if let Some(name) = self.schema.type_name_of(ty)
            && self.item_enters_depth(name)
        {
            return true;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Ref(definition) => {
                let Some(target) = self.schema.types.expect_def_output(*definition).value() else {
                    return false;
                };
                self.schema.deps.is_recursive_def(*definition)
                    || self.type_reaches_recursive_ref(target, seen)
            }
            shape => shape
                .child_type_ids()
                .any(|child| self.type_reaches_recursive_ref(child, seen)),
        }
    }
}

/// Every table-reachable analysis type sharing this item's name and shape
/// kind. Structural identity is enforced upstream; twins differ only in
/// their member-run offsets.
fn collect_twins(types: &TypeAnalysis, layout: &CaptureLayout, item: OutputItem) -> Vec<TypeId> {
    let wants_record = item.is_record();
    let mut twins = BTreeSet::new();
    for (ty, name) in types.iter_named_types() {
        if name != item.name {
            continue;
        }
        let matches_kind = match types.expect_type_shape(ty) {
            TypeShape::Record(_) => wants_record,
            TypeShape::Variant(_) => !wants_record,
            _ => false,
        };
        if !matches_kind || layout.member_base(ty).is_none() {
            continue;
        }
        twins.insert(ty);
    }
    twins.insert(item.value_type());
    twins.into_iter().collect()
}

/// Case `index`'s payload record across every twin variant type.
fn payload_twins(types: &TypeAnalysis, twins: &[TypeId], index: usize) -> Vec<TypeId> {
    twins
        .iter()
        .map(|&ty| {
            let TypeShape::Variant(cases) = types.expect_type_shape(ty) else {
                unreachable!("variant twins share the variant shape");
            };
            cases
                .values()
                .nth(index)
                .expect("twins share the case list")
                .type_id()
                .expect("payload twins belong to a payload-bearing case")
        })
        .collect()
}

/// Absolute member indices for one field/case position across all twins.
fn member_indices(layout: &CaptureLayout, twins: &[TypeId], index: usize) -> Vec<u16> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    let mut indices = twins
        .iter()
        .map(|&ty| {
            layout
                .scope(ty)
                .expect("twins are collected layout-present")
                .absolute_index(relative)
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}
