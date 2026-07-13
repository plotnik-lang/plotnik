//! Focused capture-type and producer-flow normalization.

use super::planner::CaptureTypePlanner;
use super::*;

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;

impl RawOutputGraph {
    pub(crate) fn normalize(
        self,
        types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &crate::core::Interner,
        diagnostics: &mut Diagnostics,
    ) {
        NormalizationSession::new(&self, types, interner, diagnostics).run();
    }

    fn blocked_captures(&self, raw_types: &RawTypeSnapshot) -> HashSet<RawCaptureId> {
        let mut blocked = self
            .captures
            .iter()
            .enumerate()
            .filter_map(|(index, capture)| {
                let fact = capture.observation.contract.fact;
                (!fact.is_valid() || raw_types.type_contains_invalid(fact.field().type_id))
                    .then_some(RawCaptureId(index as u32))
            })
            .collect::<HashSet<_>>();

        for alternation in self.alternations.values() {
            let Some(field) = alternation.incompatible_field else {
                continue;
            };
            let Some(output) = alternation.fields.get(&field) else {
                continue;
            };
            blocked.extend(output.producers.iter().copied());
        }
        blocked
    }

    fn omitted_captures(&self) -> HashSet<RawCaptureId> {
        let mut omitted = HashSet::new();
        for alternation in self.alternations.values() {
            for (&name, field) in &alternation.fields {
                if !alternation
                    .branches
                    .iter()
                    .any(|branch| branch.omissions.contains(&name))
                {
                    continue;
                }
                omitted.extend(field.producers.iter().copied());
            }
        }
        omitted
    }
}

struct NormalizationSession<'a, 'd> {
    graph: &'a RawOutputGraph,
    raw_types: RawTypeSnapshot,
    types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    interner: &'a crate::core::Interner,
    diagnostics: &'d mut Diagnostics,
}

impl<'a, 'd> NormalizationSession<'a, 'd> {
    fn new(
        graph: &'a RawOutputGraph,
        types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &'a crate::core::Interner,
        diagnostics: &'d mut Diagnostics,
    ) -> Self {
        Self {
            graph,
            raw_types: RawTypeSnapshot::new(types, graph),
            types,
            interner,
            diagnostics,
        }
    }

    fn run(mut self) {
        let captures = CaptureNormalizer::new(&mut self).run();
        self.types.analysis.union_flow.clear();
        let flow_count = self.graph.flows.len();
        let mut flows = FlowNormalizer::new(&mut self, &captures);
        for index in 0..flow_count {
            flows.normalize(RawFlowId(index as u32));
        }
    }
}

struct CaptureNormalizer<'s, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    blocked: HashSet<RawCaptureId>,
    omitted: HashSet<RawCaptureId>,
}

impl<'s, 'a, 'd> CaptureNormalizer<'s, 'a, 'd> {
    fn new(session: &'s mut NormalizationSession<'a, 'd>) -> Self {
        let blocked = session.graph.blocked_captures(&session.raw_types);
        let omitted = session.graph.omitted_captures();
        Self {
            session,
            blocked,
            omitted,
        }
    }

    fn run(mut self) -> HashMap<RawCaptureId, NormalizedField> {
        let mut normalized = HashMap::new();
        for index in 0..self.session.graph.captures.len() {
            let id = RawCaptureId(index as u32);
            let capture = self.session.graph.captures[index].clone();
            normalized.insert(id, self.normalize(id, &capture));
        }
        normalized
    }

    fn normalize(&mut self, id: RawCaptureId, capture: &RawCaptureOutput) -> NormalizedField {
        let raw_field = capture
            .observation
            .emitted_field
            .unwrap_or(capture.observation.contract.fact.field());
        let ordinary = NormalizedField::ordinary(raw_field, &self.session.raw_types);
        let pattern = capture.occurrence.clone();

        let RawCaptureIntent::BuiltIn { capture_type, span } = capture.observation.intent else {
            return ordinary;
        };
        if self.blocked.contains(&id) {
            return ordinary;
        }

        let mut planner = CaptureTypePlanner::new(&self.session.raw_types, self.session.types);
        let planned = match planner.plan(
            capture_type,
            capture.observation.contract,
            self.omitted.contains(&id),
        ) {
            Ok(planned) => planned,
            Err(reason) => {
                self.session
                    .diagnostics
                    .report(DiagnosticKind::InvalidCaptureType, span)
                    .detail(reason)
                    .emit();
                return ordinary;
            }
        };

        if planned.plan.suppresses_semantic_data() {
            self.session
                .diagnostics
                .report(DiagnosticKind::CaptureTypeSuppressesData, span)
                .detail(match capture_type {
                    BuiltInCaptureType::Str => {
                        "capture type `str` replaces structured data with source text"
                    }
                    BuiltInCaptureType::Bool => {
                        "capture type `bool` replaces the captured value with a boolean"
                    }
                })
                .hint("the replaced fields, variants, or scalar payload will not be returned")
                .emit();
        }

        self.session.types.analysis.capture_facts.insert(
            pattern,
            CaptureFact::built_in(
                capture.observation.contract.fact.kind(),
                capture_type,
                planned.plan,
            ),
        );
        planned.field
    }
}

#[derive(Clone)]
pub(super) struct RawTypeSnapshot {
    types: Vec<TypeShape>,
    definitions: BTreeMap<DefId, TypeId>,
    invalid_containment: HashSet<TypeId>,
}

impl RawTypeSnapshot {
    fn new(
        types: &crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        graph: &RawOutputGraph,
    ) -> Self {
        let type_shapes = types.analysis.types.clone();
        let definitions = graph
            .definitions
            .iter()
            .map(|(&def_id, &output)| (def_id, output.type_id(graph)))
            .collect();
        let invalid_containment =
            compute_invalid_containment(&type_shapes, &definitions, &types.invalid_types);
        Self {
            types: type_shapes,
            definitions,
            invalid_containment,
        }
    }

    pub(super) fn shape(&self, type_id: TypeId) -> &TypeShape {
        self.types
            .get(type_id.0 as usize)
            .expect("raw capture type must be registered")
    }

    pub(super) fn definition(&self, def_id: DefId) -> TypeId {
        *self
            .definitions
            .get(&def_id)
            .expect("raw referenced definition must have an output")
    }

    fn type_contains_invalid(&self, type_id: TypeId) -> bool {
        self.invalid_containment.contains(&type_id)
    }
}

fn compute_invalid_containment(
    types: &[TypeShape],
    definitions: &BTreeMap<DefId, TypeId>,
    invalid: &HashSet<TypeId>,
) -> HashSet<TypeId> {
    // Work backwards from invalid types. Unlike recursive DFS memoization,
    // reverse reachability remains correct when references form a cycle: each
    // containing type is visited once, after any invalid descendant reaches it.
    let mut containers = vec![Vec::new(); types.len()];
    for (index, shape) in types.iter().enumerate() {
        let container = TypeId(index as u32);
        if let TypeShape::Ref(def_id) = shape {
            let child = *definitions
                .get(def_id)
                .expect("raw referenced definition has an output");
            containers
                .get_mut(child.0 as usize)
                .expect("raw child type must be registered")
                .push(container);
            continue;
        }
        for child in shape.child_type_ids() {
            containers
                .get_mut(child.0 as usize)
                .expect("raw child type must be registered")
                .push(container);
        }
    }

    let mut contains_invalid = invalid.clone();
    let mut pending = invalid.iter().copied().collect::<Vec<_>>();
    while let Some(type_id) = pending.pop() {
        for &container in containers
            .get(type_id.0 as usize)
            .expect("invalid raw type must be registered")
        {
            if contains_invalid.insert(container) {
                pending.push(container);
            }
        }
    }
    contains_invalid
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OmissionPolicy {
    FieldOptional,
    Value(FieldFallback),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NormalizedField {
    pub(super) info: FieldInfo,
    pub(super) omission: OmissionPolicy,
}

impl NormalizedField {
    fn ordinary(info: FieldInfo, raw_types: &RawTypeSnapshot) -> Self {
        let omission =
            if !info.optional && matches!(raw_types.shape(info.type_id), TypeShape::Array { .. }) {
                OmissionPolicy::Value(FieldFallback::EmptyArray)
            } else {
                OmissionPolicy::FieldOptional
            };
        Self { info, omission }
    }

    fn omitted(
        mut self,
        types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    ) -> (Self, FieldFallback) {
        let fallback = match self.omission {
            OmissionPolicy::FieldOptional => {
                self.info = self.info.make_optional();
                FieldFallback::Null
            }
            OmissionPolicy::Value(FieldFallback::EmptyArray) => {
                let TypeShape::Array { element, .. } = types
                    .in_progress()
                    .type_shape(self.info.type_id)
                    .cloned()
                    .expect("empty-array omission requires a registered array")
                else {
                    unreachable!("empty-array omission belongs to an array field")
                };
                let array = types.intern_type(TypeShape::Array {
                    element,
                    non_empty: false,
                });
                self.info = FieldInfo::required(array);
                FieldFallback::EmptyArray
            }
            OmissionPolicy::Value(fallback @ (FieldFallback::Null | FieldFallback::False)) => {
                fallback
            }
        };
        (self, fallback)
    }
}

struct FlowNormalizer<'s, 'c, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    captures: &'c HashMap<RawCaptureId, NormalizedField>,
    normalized: HashMap<RawFlowId, BTreeMap<Symbol, NormalizedField>>,
    visiting: HashSet<RawFlowId>,
}

impl<'s, 'c, 'a, 'd> FlowNormalizer<'s, 'c, 'a, 'd> {
    fn new(
        session: &'s mut NormalizationSession<'a, 'd>,
        captures: &'c HashMap<RawCaptureId, NormalizedField>,
    ) -> Self {
        Self {
            session,
            captures,
            normalized: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    fn normalize(&mut self, id: RawFlowId) -> Option<&BTreeMap<Symbol, NormalizedField>> {
        if self.normalized.contains_key(&id) {
            return self.normalized.get(&id);
        }
        if !self.visiting.insert(id) {
            unreachable!("pattern output graph follows the finite AST, not definition refs")
        }

        let output = self.session.graph.flow(id).clone();
        let raw_fields = match &output.flow {
            RawPatternFlow::Fields(fields) => fields.clone(),
            RawPatternFlow::Void | RawPatternFlow::Value(_) => {
                self.visiting.remove(&id);
                return None;
            }
        };
        let alternation = self
            .session
            .graph
            .alternations
            .get(&output.occurrence)
            .cloned();
        let mut normalized = BTreeMap::new();
        let mut fallbacks = BTreeMap::new();

        for (&name, raw_field) in &raw_fields.fields {
            let mut field = self.normalize_sources(&output, name, raw_field);
            if let Some(alternation) = &alternation {
                let omitted = alternation
                    .branches
                    .iter()
                    .any(|branch| branch.omissions.contains(&name));
                if omitted {
                    let (omitted, fallback) = field.omitted(self.session.types);
                    field = omitted;
                    fallbacks.insert(name, fallback);
                }
            } else {
                field = self.adapt_to_raw_output(raw_field, field);
            }
            normalized.insert(name, field);
        }

        let fields = normalized
            .iter()
            .map(|(&name, field)| (name, field.info))
            .collect();
        let shape = self
            .session
            .types
            .analysis
            .types
            .get_mut(raw_fields.type_id.0 as usize)
            .expect("raw fields flow type must be registered");
        let TypeShape::Struct(current) = shape else {
            unreachable!("raw fields flow must reference a struct")
        };
        *current = fields;

        if alternation.is_some() {
            self.session
                .types
                .analysis
                .union_flow
                .insert(output.occurrence.clone(), UnionFlowPlan::new(fallbacks));
        }
        self.normalized.insert(id, normalized);
        self.visiting.remove(&id);
        self.normalized.get(&id)
    }

    fn normalize_sources(
        &mut self,
        owner: &RawPatternOutput,
        name: Symbol,
        field: &RawFieldOutput,
    ) -> NormalizedField {
        let mut sources = field.sources.iter();
        let first = *sources
            .next()
            .expect("raw public field must retain an immediate source");
        let mut normalized = self.normalize_source(first);
        if !self
            .session
            .graph
            .alternations
            .contains_key(&owner.occurrence)
        {
            return normalized;
        }
        for &source in sources {
            let other = self.normalize_source(source);
            match unify_normalized_fields(self.session.types, normalized, other) {
                Ok(unified) => normalized = unified,
                Err(()) => {
                    // Raw inference already owns structural incompatibilities.
                    // A mismatch here can only be introduced by written capture
                    // types, so report it without rewriting the raw ownership
                    // graph that subsequent fields still depend on.
                    self.session
                        .diagnostics
                        .report(
                            DiagnosticKind::IncompatibleCaptureTypes,
                            Span::new(owner.source, owner.occurrence.text_range()),
                        )
                        .detail(self.session.interner.resolve(name))
                        .emit();
                }
            }
        }
        normalized
    }

    fn normalize_source(&mut self, source: RawFieldSource) -> NormalizedField {
        match source {
            RawFieldSource::Capture(capture) => *self
                .captures
                .get(&capture)
                .expect("every raw capture has a normalized field"),
            RawFieldSource::Flow { flow, field } => *self
                .normalize(flow)
                .and_then(|fields| fields.get(&field))
                .expect("field source must survive normalization"),
        }
    }

    fn adapt_to_raw_output(
        &mut self,
        raw_output: &RawFieldOutput,
        mut field: NormalizedField,
    ) -> NormalizedField {
        let source = raw_output
            .sources
            .first()
            .copied()
            .expect("raw field has an immediate source");
        let raw_source = self.raw_source_info(source);
        if raw_source == raw_output.info {
            return field;
        }

        if raw_source.type_id == raw_output.info.type_id {
            if raw_output.info.optional && field.omission == OmissionPolicy::FieldOptional {
                field.info = field.info.make_optional();
            }
            return field;
        }

        let TypeShape::Array { element, non_empty } =
            self.session.raw_types.shape(raw_output.info.type_id)
        else {
            return field;
        };
        if *element != raw_source.type_id {
            return field;
        }
        let array = self.session.types.intern_type(TypeShape::Array {
            element: field.info.type_id,
            non_empty: *non_empty,
        });
        field.info = FieldInfo::with_optional(array, raw_output.info.optional);
        field.omission = if field.info.optional {
            OmissionPolicy::FieldOptional
        } else {
            OmissionPolicy::Value(FieldFallback::EmptyArray)
        };
        field
    }

    fn raw_source_info(&self, source: RawFieldSource) -> FieldInfo {
        match source {
            RawFieldSource::Capture(capture) => self
                .session
                .graph
                .capture(capture)
                .observation
                .emitted_field
                .expect("field-producing capture has a raw emitted field"),
            RawFieldSource::Flow { flow, field } => self
                .session
                .graph
                .flow(flow)
                .flow
                .fields()
                .and_then(|fields| fields.get(&field))
                .map(|field| field.info)
                .expect("field source must reference a raw fields flow"),
        }
    }
}

fn unify_normalized_fields(
    types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    a: NormalizedField,
    b: NormalizedField,
) -> Result<NormalizedField, ()> {
    let type_id = unify_normalized_types(types, a.info.type_id, b.info.type_id)?;
    let omission = match (a.omission, b.omission) {
        (
            OmissionPolicy::Value(FieldFallback::False),
            OmissionPolicy::Value(FieldFallback::False),
        ) => OmissionPolicy::Value(FieldFallback::False),
        (OmissionPolicy::Value(FieldFallback::Null), _)
        | (_, OmissionPolicy::Value(FieldFallback::Null)) => {
            OmissionPolicy::Value(FieldFallback::Null)
        }
        (
            OmissionPolicy::Value(FieldFallback::EmptyArray),
            OmissionPolicy::Value(FieldFallback::EmptyArray),
        ) => OmissionPolicy::Value(FieldFallback::EmptyArray),
        _ => OmissionPolicy::FieldOptional,
    };
    Ok(NormalizedField {
        info: FieldInfo::with_optional(type_id, a.info.optional || b.info.optional),
        omission,
    })
}

fn unify_normalized_types(
    types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    a: TypeId,
    b: TypeId,
) -> Result<TypeId, ()> {
    if types.types_structurally_equal(a, b) {
        return Ok(a);
    }
    let a_shape = types
        .in_progress()
        .type_shape(a)
        .cloned()
        .expect("normalized type must be registered");
    let b_shape = types
        .in_progress()
        .type_shape(b)
        .cloned()
        .expect("normalized type must be registered");
    match (a_shape, b_shape) {
        (TypeShape::Optional(a), TypeShape::Optional(b)) => {
            let inner = unify_normalized_types(types, a, b)?;
            Ok(types.intern_type(TypeShape::Optional(inner)))
        }
        (TypeShape::Optional(inner), _) => {
            let inner = unify_normalized_types(types, inner, b)?;
            Ok(types.intern_type(TypeShape::Optional(inner)))
        }
        (_, TypeShape::Optional(inner)) => {
            let inner = unify_normalized_types(types, a, inner)?;
            Ok(types.intern_type(TypeShape::Optional(inner)))
        }
        (
            TypeShape::Array {
                element: a,
                non_empty: a_non_empty,
            },
            TypeShape::Array {
                element: b,
                non_empty: b_non_empty,
            },
        ) => {
            let element = unify_normalized_types(types, a, b)?;
            Ok(types.intern_type(TypeShape::Array {
                element,
                non_empty: a_non_empty && b_non_empty,
            }))
        }
        _ => Err(()),
    }
}
