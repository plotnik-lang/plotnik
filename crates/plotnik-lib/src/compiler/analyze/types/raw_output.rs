//! Builtin-gated producer provenance for capture-type normalization.
//!
//! The public type graph intentionally collapses provenance: a record field says
//! what type is returned, not which capture occurrences produced it or which
//! alternation alternatives omitted it. Capture-type normalization needs the latter
//! facts. When a query contains a builtin capture type, this projection records
//! them at the raw inference boundary; the focused normalizer then rewrites the
//! public type graph from the exact producer and omission relationships.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::compiler::analyze::types::capture_kind::CaptureKind;
use crate::compiler::analyze::types::capture_type::{
    BuiltInCaptureType, CaptureFact, CaptureTypePlan, FieldCompletion, FieldCompletions,
    OptionMode, RawCaptureFact, TerminalData,
};
use crate::compiler::analyze::types::type_analysis::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    ListMinimum, PatternFlow, PatternShape, RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TypeId,
    TypeShape,
};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{CapturedPattern, Pattern};
use crate::core::Symbol;

mod normalize;
mod planner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawCaptureIntent {
    None,
    BuiltIn {
        capture_type: BuiltInCaptureType,
        span: Span,
    },
    Custom(Symbol),
    Invalid,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RawCaptureContract {
    fact: RawCaptureFact,
    zero_node_terminal: bool,
}

impl RawCaptureContract {
    pub(crate) fn new(fact: RawCaptureFact, zero_node_terminal: bool) -> Self {
        Self {
            fact,
            zero_node_terminal,
        }
    }
}

/// One regular capture as it existed before a built-in capture type was
/// applied. `emitted_field` is separate from validity: error recovery may keep
/// a plausible field in the public shape, while a duplicate bubbling capture
/// is rejected before it reaches that shape.
#[derive(Clone, Debug)]
pub(crate) struct RawCaptureObservation {
    name: Symbol,
    contract: RawCaptureContract,
    intent: RawCaptureIntent,
    emitted_field: Option<RecordField>,
}

impl RawCaptureObservation {
    pub(crate) fn new(
        name: Symbol,
        contract: RawCaptureContract,
        intent: RawCaptureIntent,
    ) -> Self {
        Self {
            name,
            contract,
            intent,
            emitted_field: None,
        }
    }

    pub(crate) fn emitting(mut self, field: RecordField) -> Self {
        self.emitted_field = Some(field);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RawCaptureId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RawFlowId(u32);

#[derive(Clone, Debug)]
struct RawCaptureOutput {
    occurrence: CapturedPattern,
    source: SourceId,
    observation: RawCaptureObservation,
}

#[derive(Clone, Debug)]
struct RawFieldOutput {
    info: RecordField,
    producers: Vec<RawCaptureId>,
    sources: Vec<RawFieldSource>,
}

#[derive(Clone, Copy, Debug)]
enum RawFieldSource {
    Capture(RawCaptureId),
    Flow { flow: RawFlowId, field: Symbol },
}

#[derive(Clone, Debug)]
struct RawFieldsFlow {
    type_id: TypeId,
    fields: BTreeMap<Symbol, RawFieldOutput>,
}

#[derive(Clone, Debug)]
enum RawPatternFlow {
    NoValue,
    Value,
    Fields(RawFieldsFlow),
}

impl RawPatternFlow {
    fn fields(&self) -> Option<&BTreeMap<Symbol, RawFieldOutput>> {
        match self {
            Self::Fields(fields) => Some(&fields.fields),
            Self::NoValue | Self::Value => None,
        }
    }
}

#[derive(Clone, Debug)]
struct RawPatternOutput {
    occurrence: Pattern,
    source: SourceId,
    flow: RawPatternFlow,
}

#[derive(Clone, Debug)]
struct RawAlternationField {
    producers: BTreeSet<RawCaptureId>,
}

#[derive(Clone, Debug)]
struct RawAlternationAlternative {
    omissions: BTreeSet<Symbol>,
}

#[derive(Clone, Debug)]
struct RawAlternationOutput {
    fields: BTreeMap<Symbol, RawAlternationField>,
    alternatives: Vec<RawAlternationAlternative>,
    incompatible_field: Option<Symbol>,
}

/// Frozen, builtin-only provenance projection read by the capture-type
/// normalizer. It never leaks raw producer identities into public output.
#[derive(Clone, Debug)]
pub(crate) struct RawOutputGraph {
    captures: Vec<RawCaptureOutput>,
    flows: Vec<RawPatternOutput>,
    capture_producer_ids_by_record_type: HashMap<TypeId, BTreeSet<RawCaptureId>>,
    alternations: HashMap<Pattern, RawAlternationOutput>,
}

impl RawOutputGraph {
    fn capture(&self, id: RawCaptureId) -> &RawCaptureOutput {
        self.captures
            .get(id.0 as usize)
            .expect("raw capture id must reference a recorded occurrence")
    }

    fn flow(&self, id: RawFlowId) -> &RawPatternOutput {
        self.flows
            .get(id.0 as usize)
            .expect("raw flow id must reference a recorded pattern")
    }

    fn capture_producer_ids_for_record_type(
        &self,
        record_type_id: TypeId,
    ) -> BTreeSet<RawCaptureId> {
        self.capture_producer_ids_by_record_type
            .get(&record_type_id)
            .cloned()
            .unwrap_or_default()
    }
}
/// Mutable recorder used only by the raw inference builder.
#[derive(Default)]
pub(crate) struct RawOutputGraphBuilder {
    captures: Vec<RawCaptureOutput>,
    capture_ids: HashMap<Pattern, RawCaptureId>,
    flows: Vec<RawPatternOutput>,
    flow_ids: HashMap<Pattern, RawFlowId>,
    alternations: HashMap<Pattern, RawAlternationOutput>,
    incompatibilities: HashMap<Pattern, Symbol>,
}

impl RawOutputGraphBuilder {
    pub(crate) fn record_capture(
        &mut self,
        captured_pattern: CapturedPattern,
        source: SourceId,
        observation: RawCaptureObservation,
    ) {
        let pattern_key = Pattern::CapturedPattern(captured_pattern.clone());
        let output = RawCaptureOutput {
            occurrence: captured_pattern,
            source,
            observation,
        };
        if let Some(&id) = self.capture_ids.get(&pattern_key) {
            self.captures[id.0 as usize] = output;
            return;
        }

        let id = RawCaptureId(self.captures.len() as u32);
        self.capture_ids.insert(pattern_key, id);
        self.captures.push(output);
    }

    pub(crate) fn record_pattern(
        &mut self,
        occurrence: Pattern,
        source: SourceId,
        shape: &PatternShape,
        analysis: &TypeAnalysis,
    ) {
        // This is a boundary projection, not a second inference result. Child
        // flows have already been recorded, so one immediate-child walk adds
        // producer identities to the accepted PatternShape. The builder is
        // enabled only for queries whose cheap pre-scan found a builtin.
        let flow = match &shape.flow {
            PatternFlow::NoValue => RawPatternFlow::NoValue,
            PatternFlow::Value(_) => RawPatternFlow::Value,
            PatternFlow::Fields(type_id) => {
                let mut sources = self.pattern_field_sources(&occurrence);
                let fields = analysis
                    .expect_record_fields(*type_id)
                    .iter()
                    .map(|(&name, &info)| {
                        let sources = sources
                            .remove(&name)
                            .unwrap_or_else(|| panic!("raw field must retain a capture producer"));
                        let producers = self.flatten_producers(&sources);
                        (
                            name,
                            RawFieldOutput {
                                info,
                                producers,
                                sources,
                            },
                        )
                    })
                    .collect();
                RawPatternFlow::Fields(RawFieldsFlow {
                    type_id: *type_id,
                    fields,
                })
            }
        };

        if let Some(&id) = self.flow_ids.get(&occurrence) {
            self.flows[id.0 as usize] = RawPatternOutput {
                occurrence: occurrence.clone(),
                source,
                flow,
            };
        } else {
            let id = RawFlowId(self.flows.len() as u32);
            self.flow_ids.insert(occurrence.clone(), id);
            self.flows.push(RawPatternOutput {
                occurrence: occurrence.clone(),
                source,
                flow,
            });
        }

        if matches!(occurrence, Pattern::Alternation(_)) {
            self.record_alternation(occurrence);
        }
    }

    pub(crate) fn record_alternation_incompatibility(&mut self, pattern: Pattern, field: Symbol) {
        self.incompatibilities.insert(pattern, field);
    }

    pub(crate) fn finish(self) -> RawOutputGraph {
        let mut capture_producer_ids_by_record_type: HashMap<TypeId, BTreeSet<RawCaptureId>> =
            HashMap::new();
        for output in &self.flows {
            let RawPatternFlow::Fields(flow) = &output.flow else {
                continue;
            };
            let producer_ids = capture_producer_ids_by_record_type
                .entry(flow.type_id)
                .or_default();
            for field in flow.fields.values() {
                producer_ids.extend(field.producers.iter().copied());
            }
        }
        RawOutputGraph {
            captures: self.captures,
            flows: self.flows,
            capture_producer_ids_by_record_type,
            alternations: self.alternations,
        }
    }

    fn pattern_field_sources(&self, pattern: &Pattern) -> BTreeMap<Symbol, Vec<RawFieldSource>> {
        let mut sources = BTreeMap::new();

        if let Pattern::CapturedPattern(_) = pattern
            && let Some(&capture_id) = self.capture_ids.get(pattern)
        {
            let capture = &self.captures[capture_id.0 as usize];
            if capture.observation.contract.fact.kind() == CaptureKind::Node {
                self.extend_child_sources(&mut sources, pattern);
            }
            if capture.observation.emitted_field.is_some() {
                sources
                    .entry(capture.observation.name)
                    .or_default()
                    .push(RawFieldSource::Capture(capture_id));
            }
            return sources;
        }

        self.extend_child_sources(&mut sources, pattern);
        sources
    }

    fn extend_child_sources(
        &self,
        target: &mut BTreeMap<Symbol, Vec<RawFieldSource>>,
        pattern: &Pattern,
    ) {
        for_each_inference_child(pattern, |child| {
            let flow_id = self.flow_id(&child);
            let child_flow = &self.flow(flow_id).flow;
            let Some(fields) = child_flow.fields() else {
                return;
            };
            for &name in fields.keys() {
                target.entry(name).or_default().push(RawFieldSource::Flow {
                    flow: flow_id,
                    field: name,
                });
            }
        });
    }

    fn flatten_producers(&self, sources: &[RawFieldSource]) -> Vec<RawCaptureId> {
        let mut producers = BTreeSet::new();
        for source in sources {
            match *source {
                RawFieldSource::Capture(capture) => {
                    producers.insert(capture);
                }
                RawFieldSource::Flow { flow, field } => {
                    let fields = self
                        .flow(flow)
                        .flow
                        .fields()
                        .expect("field source must reference a fields flow");
                    for &producer in &fields[&field].producers {
                        producers.insert(producer);
                    }
                }
            }
        }
        producers.into_iter().collect()
    }

    fn record_alternation(&mut self, pattern: Pattern) {
        let alternative_flows = alternation_bodies(&pattern)
            .into_iter()
            .map(|body| body.map(|body| self.flow_id(&body)))
            .collect::<Vec<_>>();

        let mut fields: BTreeMap<Symbol, RawAlternationField> = BTreeMap::new();
        for &flow in alternative_flows.iter().flatten() {
            let Some(alternative_fields) = self.flow(flow).flow.fields() else {
                continue;
            };
            for (&name, field) in alternative_fields {
                let output = fields.entry(name).or_insert_with(|| RawAlternationField {
                    producers: BTreeSet::new(),
                });
                for &producer in &field.producers {
                    output.producers.insert(producer);
                }
            }
        }

        let all_fields = fields.keys().copied().collect::<BTreeSet<_>>();
        let alternatives = alternative_flows
            .into_iter()
            .map(|flow| {
                let present = flow
                    .and_then(|flow| self.flow(flow).flow.fields())
                    .map(|fields| fields.keys().copied().collect::<BTreeSet<_>>())
                    .unwrap_or_default();
                let omissions = all_fields.difference(&present).copied().collect();
                RawAlternationAlternative { omissions }
            })
            .collect();

        let output = RawAlternationOutput {
            fields,
            alternatives,
            incompatible_field: self.incompatibilities.get(&pattern).copied(),
        };
        self.alternations.insert(pattern, output);
    }

    fn flow(&self, id: RawFlowId) -> &RawPatternOutput {
        self.flows
            .get(id.0 as usize)
            .expect("raw flow id must reference a recorded pattern")
    }

    fn flow_id(&self, pattern: &Pattern) -> RawFlowId {
        *self
            .flow_ids
            .get(pattern)
            .expect("child pattern must be inferred before its parent raw flow")
    }
}

/// Immediate children visited by raw inference for each pattern role. Keeping
/// this topology mirror local avoids threading optional provenance through the
/// no-builtin inference result and cache.
fn for_each_inference_child(pattern: &Pattern, mut visit: impl FnMut(Pattern)) {
    match pattern {
        Pattern::Alternation(alternation) => {
            for child in alternation
                .alternatives()
                .filter_map(|alternative| alternative.body())
                .chain(alternation.patterns())
            {
                visit(child);
            }
        }
        _ => {
            for child in pattern.children() {
                visit(child);
            }
        }
    }
}

fn alternation_bodies(pattern: &Pattern) -> Vec<Option<Pattern>> {
    match pattern {
        Pattern::Alternation(alternation) => alternation
            .alternatives()
            .map(|alternative| alternative.body())
            .chain(alternation.patterns().map(Some))
            .collect(),
        _ => unreachable!("raw alternation output requires an alternation pattern"),
    }
}
