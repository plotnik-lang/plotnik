//! Target-neutral typed result-decoding plan.
//!
//! Match-journal events name record fields and variant cases with absolute layout
//! slots. This plan resolves every nominal twin to the complete set of slots
//! it may produce and turns result types into a small value-decoding algebra.
//! Backends choose identifiers, error syntax, recursion representation, and
//! construction syntax without walking analysis types or rebuilding tables.

use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::result::{
    CaptureLayout, CaptureMemberKind, PublicResultGroup, ResultItem, ResultItemKind, ResultSchema,
};
use crate::compiler::analyze::types::type_shape::{DefinitionOutput, TypeId, TypeShape};
use crate::compiler::ids::ResultMemberId;
use crate::core::Symbol;

#[derive(Clone, Debug)]
pub(crate) struct ResultDecodePlan {
    items: Vec<DecodeItem>,
    items_by_name: HashMap<Symbol, usize>,
}

impl ResultDecodePlan {
    pub(super) fn build(schema: &ResultSchema<'_>) -> Self {
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
            .expect("every decode target declares a result item");
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
    pub(crate) indices: Vec<ResultMemberId>,
    pub(crate) value: DecodeValue,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeCase {
    pub(crate) name: Symbol,
    /// Every absolute `VariantOpen` slot this case may use across twins.
    pub(crate) indices: Vec<ResultMemberId>,
    pub(crate) payload: Option<DecodeScope>,
}

#[derive(Clone, Debug)]
pub(crate) enum DecodeValue {
    Node,
    Text,
    Bool,
    Option(Box<DecodeValue>),
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
    schema: &'p ResultSchema<'a>,
}

impl DecodePlanBuilder<'_, '_> {
    fn item(&self, item: ResultItem) -> DecodeItem {
        let kind = match item.kind {
            ResultItemKind::Record => {
                let group = self.schema.public_result_group(item);
                DecodeItemKind::Record(self.scope(group.representative(), group.occurrences()))
            }
            ResultItemKind::Variant => {
                let group = self.schema.public_result_group(item);
                DecodeItemKind::Variant(self.variant_cases(group))
            }
            ResultItemKind::Alias => DecodeItemKind::Alias(self.value(item.value_type())),
            ResultItemKind::MatchOnlyDef => DecodeItemKind::MatchOnlyDefinition,
        };
        DecodeItem {
            name: item.name,
            output: item.output,
            kind,
            enters_depth: self.item_enters_depth(item.name),
            fallible: self.item_is_fallible(item),
        }
    }

    fn variant_cases(&self, group: &PublicResultGroup) -> Vec<DecodeCase> {
        let layout = self.schema.layout();
        let scope = layout
            .scope(group.representative())
            .expect("variant result item has a capture scope");
        scope
            .members()
            .enumerate()
            .map(|(index, member)| {
                let descriptor = layout.expect_member(member);
                let CaptureMemberKind::Case(payload) = descriptor.kind else {
                    unreachable!("variant result scope contains only cases");
                };
                let payload = payload.type_id().map(|payload| {
                    let payloads = payload_occurrences(layout, group.occurrences(), index);
                    self.scope(payload, &payloads)
                });
                DecodeCase {
                    name: descriptor.name,
                    indices: member_indices(layout, group.occurrences(), index),
                    payload,
                }
            })
            .collect()
    }

    fn scope(&self, type_id: TypeId, twins: &[TypeId]) -> DecodeScope {
        let layout = self.schema.layout();
        let scope = layout
            .scope(type_id)
            .expect("record decode scope has a capture layout scope");
        DecodeScope {
            fields: scope
                .members()
                .enumerate()
                .map(|(index, member)| {
                    let descriptor = layout.expect_member(member);
                    let CaptureMemberKind::Field(info) = descriptor.kind else {
                        unreachable!("record result scope contains only fields");
                    };
                    DecodeField {
                        name: descriptor.name,
                        indices: member_indices(layout, twins, index),
                        value: self.value(info.final_type),
                    }
                })
                .collect(),
        }
    }

    fn value(&self, ty: TypeId) -> DecodeValue {
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node => DecodeValue::Node,
            TypeShape::Text => DecodeValue::Text,
            TypeShape::Bool => DecodeValue::Bool,
            TypeShape::Option(inner) => DecodeValue::Option(Box::new(self.value(*inner))),
            TypeShape::List { element, .. } => DecodeValue::List(Box::new(self.value(*element))),
            TypeShape::Record(_) | TypeShape::Variant(_) => DecodeValue::Nested {
                item: self
                    .schema
                    .types
                    .type_name_of(ty)
                    .expect("naming pass names every non-payload composite"),
                source_type: ty,
            },
            TypeShape::Ref(declaration) => {
                if self.schema.types.declaration_body(*declaration).is_none() {
                    return DecodeValue::Node;
                }
                DecodeValue::Nested {
                    item: self.schema.types.declaration_name(*declaration),
                    source_type: ty,
                }
            }
        }
    }

    fn item_enters_depth(&self, name: Symbol) -> bool {
        self.schema
            .definitions
            .id_for_symbol(name)
            .is_some_and(|definition| self.schema.definitions.is_recursive(definition))
    }

    fn item_is_fallible(&self, item: ResultItem) -> bool {
        self.item_enters_depth(item.name)
            || item.output.value().is_some_and(|type_id| {
                self.type_reaches_recursive_ref(type_id, &mut HashSet::new())
            })
    }

    fn type_reaches_recursive_ref(&self, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        if let Some(name) = self.schema.types.type_name_of(ty)
            && self.item_enters_depth(name)
        {
            return true;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Ref(declaration) => {
                let Some(target) = self.schema.types.declaration_body(*declaration) else {
                    return false;
                };
                self.schema
                    .types
                    .declaration_definition(*declaration)
                    .is_some_and(|definition| self.schema.definitions.is_recursive(definition))
                    || self.type_reaches_recursive_ref(target, seen)
            }
            shape => shape
                .child_type_ids()
                .any(|child| self.type_reaches_recursive_ref(child, seen)),
        }
    }
}

/// Case `index`'s payload record across every nominal variant occurrence.
fn payload_occurrences(
    layout: &CaptureLayout,
    occurrences: &[TypeId],
    index: usize,
) -> Vec<TypeId> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    occurrences
        .iter()
        .map(|&ty| {
            let member = layout
                .scope(ty)
                .and_then(|scope| scope.member_id(relative))
                .expect("variant twins share the case list");
            let CaptureMemberKind::Case(payload) = layout.expect_member(member).kind else {
                unreachable!("variant twin scopes contain only cases");
            };
            payload
                .type_id()
                .expect("payload twins belong to a payload-bearing case")
        })
        .collect()
}

/// Absolute member indices for one field/case position across all occurrences.
fn member_indices(
    layout: &CaptureLayout,
    occurrences: &[TypeId],
    index: usize,
) -> Vec<ResultMemberId> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    let mut indices = occurrences
        .iter()
        .map(|&ty| {
            layout
                .scope(ty)
                .expect("twins are collected layout-present")
                .member_id(relative)
                .expect("twins share the member list")
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}
