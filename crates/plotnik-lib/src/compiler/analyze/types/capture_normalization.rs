use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::types::capture_type::{BuiltInCaptureType, RawCaptureFact};
use crate::compiler::analyze::types::inference_flow::CaptureId;
use crate::compiler::analyze::types::type_shape::RecordField;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::CapturedPattern;
use crate::core::Symbol;

mod normalize;
mod planner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureTypeIntent {
    None,
    BuiltIn {
        capture_type: BuiltInCaptureType,
        span: Span,
    },
    Custom(Symbol),
    Invalid,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureContract {
    pub(super) fact: RawCaptureFact,
    pub(super) zero_node_terminal: bool,
}

impl CaptureContract {
    pub(crate) fn new(fact: RawCaptureFact, zero_node_terminal: bool) -> Self {
        Self {
            fact,
            zero_node_terminal,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureObservation {
    pub(super) name: Symbol,
    pub(super) contract: CaptureContract,
    pub(super) intent: CaptureTypeIntent,
    pub(super) produced_field: Option<RecordField>,
}

impl CaptureObservation {
    pub(crate) fn new(name: Symbol, contract: CaptureContract, intent: CaptureTypeIntent) -> Self {
        Self {
            name,
            contract,
            intent,
            produced_field: None,
        }
    }

    pub(crate) fn producing(mut self, field: RecordField) -> Self {
        self.produced_field = Some(field);
        self
    }
}

#[derive(Clone, Debug)]
pub(super) struct RecordedCapture {
    pub(super) occurrence: Located<CapturedPattern>,
    pub(super) observation: CaptureObservation,
}

#[derive(Default)]
pub(crate) struct CaptureProvenance {
    pub(super) captures: Vec<RecordedCapture>,
    pub(super) blocked_capture_ids: HashSet<CaptureId>,
}

impl CaptureProvenance {
    pub(crate) fn record_capture(
        &mut self,
        occurrence: Located<CapturedPattern>,
        observation: CaptureObservation,
    ) -> CaptureId {
        let id = CaptureId::from_index(self.captures.len());
        self.captures.push(RecordedCapture {
            occurrence,
            observation,
        });
        id
    }

    pub(crate) fn block_captures(&mut self, capture_ids: impl IntoIterator<Item = CaptureId>) {
        self.blocked_capture_ids.extend(capture_ids);
    }

    pub(crate) fn has_built_in_capture_types(&self) -> bool {
        self.captures.iter().any(|capture| {
            matches!(
                capture.observation.intent,
                CaptureTypeIntent::BuiltIn { .. }
            )
        })
    }
}
