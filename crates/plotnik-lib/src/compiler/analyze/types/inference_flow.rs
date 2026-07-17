use std::collections::{BTreeMap, BTreeSet};

use crate::compiler::analyze::types::type_shape::{RecordField, TypeId};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Pattern;
use crate::core::Symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct CaptureId(u32);

impl CaptureId {
    pub(super) fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).expect("capture count fits u32"))
    }

    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub(super) enum FieldSource {
    Capture {
        capture_id: CaptureId,
        capture_span: Span,
        name_span: Span,
        info: RecordField,
    },
    Forwarded {
        pattern: Pattern,
        field: Symbol,
        capture_span: Span,
        name_span: Span,
        info: RecordField,
    },
}

impl FieldSource {
    pub(super) fn capture_span(&self) -> Span {
        match self {
            Self::Capture { capture_span, .. } | Self::Forwarded { capture_span, .. } => {
                *capture_span
            }
        }
    }

    pub(super) fn info(&self) -> RecordField {
        match self {
            Self::Capture { info, .. } | Self::Forwarded { info, .. } => *info,
        }
    }

    pub(super) fn name_span(&self) -> Span {
        match self {
            Self::Capture { name_span, .. } | Self::Forwarded { name_span, .. } => *name_span,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct InferredField {
    pub(super) info: RecordField,
    pub(super) producers: BTreeSet<CaptureId>,
    pub(super) sources: Vec<FieldSource>,
}

impl InferredField {
    pub(super) fn capture(
        info: RecordField,
        capture_id: CaptureId,
        name_span: Span,
        capture_span: Span,
    ) -> Self {
        Self {
            info,
            producers: BTreeSet::from([capture_id]),
            sources: vec![FieldSource::Capture {
                capture_id,
                capture_span,
                name_span,
                info,
            }],
        }
    }

    pub(super) fn forwarded(
        info: RecordField,
        pattern: Pattern,
        field: Symbol,
        source: &Self,
    ) -> Self {
        Self {
            info,
            producers: source.producers.clone(),
            sources: vec![FieldSource::Forwarded {
                pattern,
                field,
                capture_span: source.first_capture_span(),
                name_span: source.first_name_span(),
                info,
            }],
        }
    }

    pub(super) fn first_name_span(&self) -> Span {
        self.sources
            .first()
            .expect("inferred result field retains an immediate source")
            .name_span()
    }

    fn first_capture_span(&self) -> Span {
        self.sources
            .first()
            .expect("inferred result field retains an immediate source")
            .capture_span()
    }
}

#[derive(Clone, Debug)]
pub(super) struct InferredFieldFlow {
    pub(super) type_id: TypeId,
    pub(super) fields: BTreeMap<Symbol, InferredField>,
    pub(super) alternation_omissions: Option<BTreeSet<Symbol>>,
}

impl InferredFieldFlow {
    pub(super) fn new(type_id: TypeId, fields: BTreeMap<Symbol, InferredField>) -> Self {
        Self {
            type_id,
            fields,
            alternation_omissions: None,
        }
    }

    pub(super) fn alternation(
        type_id: TypeId,
        fields: BTreeMap<Symbol, InferredField>,
        alternation_omissions: BTreeSet<Symbol>,
    ) -> Self {
        Self {
            type_id,
            fields,
            alternation_omissions: Some(alternation_omissions),
        }
    }

    pub(super) fn forwarded(pattern: Pattern, source: &Self) -> Self {
        let fields = source
            .fields
            .iter()
            .map(|(&name, field)| {
                (
                    name,
                    InferredField::forwarded(field.info, pattern.clone(), name, field),
                )
            })
            .collect();
        Self::new(source.type_id, fields)
    }
}
