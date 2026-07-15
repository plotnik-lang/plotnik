//! Conservative native-stack estimate for generated typed result decoders.
//!
//! Rust does not expose final stack maps during source generation. This model
//! counts the locals emitted for each decode shape; `plotnik-rt` adds runtime
//! padding when it converts the largest frame into an automatic depth limit.

use std::collections::{BTreeMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{CasePayload, RecordField, TypeId, TypeShape};
use crate::compiler::emit::plan::{DecodeItem, ResultDecodePlan};
use crate::compiler::emit::targets::rust::{TypeContext, TypeModel};
use crate::core::Symbol;

const WORD_BYTES: u64 = 8;
const VEC_VALUE_BYTES: u64 = 24;
const OPTION_TAG_BYTES: u64 = 8;
const DECODER_FRAME_BASE_BYTES: u64 = 128;

pub(super) struct DecoderFrameEstimator<'m, 'a> {
    model: &'m TypeModel<'a>,
    types: &'a TypeAnalysis,
    decode: &'a ResultDecodePlan,
}

impl<'m, 'a> DecoderFrameEstimator<'m, 'a> {
    pub(super) fn new(model: &'m TypeModel<'a>, decode: &'a ResultDecodePlan) -> Self {
        Self {
            model,
            types: model.schema().types,
            decode,
        }
    }

    pub(super) fn max_bytes(&self) -> u64 {
        self.decode
            .items()
            .iter()
            .filter(|item| item.has_decoder())
            .map(|item| self.decoder_frame_bytes(item))
            .max()
            .unwrap_or(DECODER_FRAME_BASE_BYTES)
    }

    fn decoder_frame_bytes(&self, item: &DecodeItem) -> u64 {
        let item_ty = item.value_type();
        let guard_bytes = if item.fallible { WORD_BYTES } else { 0 };
        let local_bytes = match self.types.expect_type_shape(item_ty) {
            TypeShape::Record(fields) => self.field_scope_frame_bytes(item_ty, fields),
            TypeShape::Variant(cases) => cases
                .values()
                .map(|&payload| self.variant_payload_frame_bytes(item_ty, payload))
                .max()
                .unwrap_or(0),
            _ => self.value_temp_bytes(item_ty, TypeContext::item(item_ty)),
        };

        DECODER_FRAME_BASE_BYTES
            .saturating_add(guard_bytes)
            .saturating_add(local_bytes)
    }

    fn variant_payload_frame_bytes(&self, owner: TypeId, payload: CasePayload) -> u64 {
        let Some(payload) = payload.type_id() else {
            return 0;
        };
        let TypeShape::Record(fields) = self.types.expect_type_shape(payload) else {
            unreachable!("variant case has no payload or an anonymous record payload");
        };
        self.field_scope_frame_bytes(owner, fields)
    }

    fn field_scope_frame_bytes(
        &self,
        owner: TypeId,
        fields: &BTreeMap<Symbol, RecordField>,
    ) -> u64 {
        let context = TypeContext::item(owner);
        let slots = fields
            .values()
            .map(|info| self.option_value_bytes(self.field_value_bytes(info, context)))
            .fold(0_u64, u64::saturating_add);
        let widest_assignment = fields
            .values()
            .map(|info| self.field_value_bytes(info, context))
            .max()
            .unwrap_or(0);
        slots.saturating_add(widest_assignment)
    }

    fn field_value_bytes(&self, info: &RecordField, context: TypeContext) -> u64 {
        self.field_value_bytes_seen(info, context, &mut HashSet::new())
    }

    fn field_value_bytes_seen(
        &self,
        info: &RecordField,
        context: TypeContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        self.type_value_bytes(info.final_type, context, seen)
    }

    fn value_temp_bytes(&self, ty: TypeId, context: TypeContext) -> u64 {
        match self.types.expect_type_shape(ty) {
            TypeShape::List { element, .. } => VEC_VALUE_BYTES.saturating_add(
                self.type_value_bytes(*element, context.list_element(), &mut HashSet::new()),
            ),
            _ => self.type_value_bytes(ty, context, &mut HashSet::new()),
        }
    }

    fn type_value_bytes(
        &self,
        ty: TypeId,
        context: TypeContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        if !seen.insert(ty) {
            return WORD_BYTES;
        }

        match self.types.expect_type_shape(ty) {
            TypeShape::Node => plotnik_rt::GENERATED_NODE_VALUE_BYTES,
            TypeShape::Text => 2 * WORD_BYTES,
            TypeShape::Bool => 1,
            TypeShape::Option(inner) => {
                let inner = self.type_value_bytes(*inner, context, seen);
                self.option_value_bytes(inner)
            }
            TypeShape::List { element, .. } => VEC_VALUE_BYTES
                .saturating_add(self.type_value_bytes(*element, context.list_element(), seen)),
            TypeShape::Record(fields) => fields
                .values()
                .map(|info| {
                    let mut field_seen = seen.clone();
                    self.field_value_bytes_seen(info, context, &mut field_seen)
                })
                .fold(0_u64, u64::saturating_add),
            TypeShape::Variant(cases) => {
                let widest = cases
                    .values()
                    .map(|&payload| {
                        let Some(payload) = payload.type_id() else {
                            return 0;
                        };
                        let mut variant_seen = seen.clone();
                        self.type_value_bytes(payload, context, &mut variant_seen)
                    })
                    .max()
                    .unwrap_or(0);
                WORD_BYTES.saturating_add(widest)
            }
            TypeShape::Ref(declaration) => {
                let Some(target) = self.types.declaration_body(*declaration) else {
                    return plotnik_rt::GENERATED_NODE_VALUE_BYTES;
                };
                if self.model.is_boxed_ref(context, ty) {
                    return WORD_BYTES;
                }
                self.type_value_bytes(target, context, seen)
            }
        }
    }

    fn option_value_bytes(&self, value_bytes: u64) -> u64 {
        align_to_word(value_bytes.saturating_add(OPTION_TAG_BYTES))
    }
}

fn align_to_word(bytes: u64) -> u64 {
    let rem = bytes % WORD_BYTES;
    if rem == 0 {
        return bytes;
    }
    bytes.saturating_add(WORD_BYTES - rem)
}
