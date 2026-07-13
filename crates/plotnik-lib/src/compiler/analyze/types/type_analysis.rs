//! `TypeAnalysis`: the frozen result of type inference.
//!
//! Holds the interned type registry, each definition's output type and root
//! extent, the per-pattern inference results, and explicit type aliases. It is built
//! incrementally by [`TypeAnalysisBuilder`] and frozen with
//! [`TypeAnalysisBuilder::finish`]; past that boundary it is immutable and its
//! accessors are trusted (a structural miss is a compiler bug, not a query
//! condition).
//!
//! Definition *identity* (names, `DefId`s, recursion) is not stored here — it is
//! owned by `DependencyAnalysis` and read from there. This artifact only maps the
//! `DefId`s that analysis already assigned to the types it inferred for them.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compiler::analyze::types::raw_output::{
    RawCaptureObservation, RawDefinitionValueRole, RawOutputGraphBuilder,
};
use crate::compiler::analyze::types::type_shape::{
    PatternFlow, PatternShape, RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TYPE_VOID, TypeId,
    TypeShape,
};
use crate::compiler::analyze::types::{CaptureFact, FieldCompletions, RootExtent};
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::Pattern;
use crate::core::Symbol;

/// One custom `:: TypeName` occurrence, recorded during inference for the
/// naming pass to validate (nominal identity, collisions, redundancy).
#[derive(Clone, Copy, Debug)]
pub struct CustomCaptureTypeOccurrence {
    pub name: Symbol,
    pub span: Span,
    pub type_id: TypeId,
}

/// Frozen registry of inferred types and per-definition / per-pattern results.
///
/// Constructed only via [`TypeAnalysisBuilder`]; the private fields and
/// `#[non_exhaustive]` keep that the single entry point.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TypeAnalysis {
    pub(super) types: Vec<TypeShape>,

    /// Each definition's output type, keyed by `DefId`. `BTreeMap` so iteration
    /// is in `DefId` order — the SCC/emission order entrypoints rely on. Total
    /// over every scheduled definition: a captureless structural body maps to
    /// `TYPE_VOID`, so a lookup for any admitted `DefId` hits. `finish` admits
    /// the map only after checking its type ids and every `Ref` target are
    /// consistent.
    pub(super) def_output: BTreeMap<DefId, TypeId>,

    /// Each definition's static top-level extent. `SingleNode` definitions are
    /// selectable as entry points; all others remain reusable fragments.
    def_root_extent: BTreeMap<DefId, RootExtent>,

    pub(super) pattern_result: HashMap<Pattern, PatternShape>,

    /// Raw capture mechanism plus the optional built-in capture-type plan for
    /// every admitted regular capture occurrence.
    pub(super) capture_facts: HashMap<Pattern, CaptureFact>,

    /// Final completion behavior for every merged field of each alternation.
    pub(super) field_completions: HashMap<Pattern, FieldCompletions>,

    /// Structural bodies that are referenced by a generated or explicit type
    /// name. Definition declarations are keyed by `DefId` instead: their names
    /// must not attach to structurally interned bodies shared with unrelated
    /// positions. `BTreeMap` preserves deterministic body order.
    named_types: BTreeMap<TypeId, Symbol>,
}

impl TypeAnalysis {
    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.types.get(id.0 as usize)
    }

    pub fn expect_type_shape(&self, id: TypeId) -> &TypeShape {
        self.type_shape(id)
            .expect("admitted type id must reference a registered type")
    }

    fn type_is_option(&self, mut type_id: TypeId) -> bool {
        let mut seen = HashSet::new();
        while seen.insert(type_id) {
            match self.type_shape(type_id) {
                Some(TypeShape::Option(_)) => return true,
                Some(TypeShape::Ref(def_id)) => {
                    let Some(target) = self.def_output(*def_id) else {
                        return false;
                    };
                    type_id = target;
                }
                _ => return false,
            }
        }
        false
    }

    pub fn record_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, RecordField>> {
        match self.type_shape(id)? {
            TypeShape::Record(fields) => Some(fields),
            _ => None,
        }
    }

    /// Fields of the record a `Fields` flow points to.
    ///
    /// Every `PatternFlow::Fields` is constructed by interning a `Record` (see the
    /// `intern_record`/`intern_single_field_record` calls at every `Fields` construction
    /// site), so a non-`Record` id here is a broken type-system invariant, not a
    /// runtime condition the query can trigger. We surface it loudly instead of
    /// fabricating an empty record that would silently mistype the output.
    pub fn expect_record_fields(&self, id: TypeId) -> &BTreeMap<Symbol, RecordField> {
        match self.expect_type_shape(id) {
            TypeShape::Record(fields) => fields,
            _ => panic!("Fields flow must point to a Record type"),
        }
    }

    /// Whether a type is a meaningful structured output (variant/record, or a
    /// list/option thereof). Plain `Node` is not — it is the matched node,
    /// captured directly.
    ///
    /// A `Ref` resolves through its target: a reference to a void definition
    /// leaves no pending value at runtime (the capture takes the matched node),
    /// so it must not classify as structured. Mid-inference a same-SCC target
    /// has no output yet; assume structured — the admitted classification that
    /// lowering reads always resolves.
    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        match self.type_shape(type_id) {
            Some(TypeShape::Variant(_) | TypeShape::Record(_)) => true,
            Some(TypeShape::Ref(def_id)) => self
                .def_output(*def_id)
                .is_none_or(|t| self.is_structured_output(t)),
            Some(shape @ (TypeShape::Array { .. } | TypeShape::Option(_))) => shape
                .child_type_ids()
                .any(|id| id != TYPE_NODE && self.is_structured_output(id)),
            _ => false,
        }
    }

    pub fn pattern_result(&self, pattern: &Pattern) -> Option<&PatternShape> {
        self.pattern_result.get(pattern)
    }

    pub fn expect_pattern_result(&self, pattern: &Pattern) -> &PatternShape {
        self.pattern_result(pattern)
            .expect("admitted pattern must have an inferred result")
    }

    pub fn capture_fact(&self, pattern: &Pattern) -> Option<&CaptureFact> {
        self.capture_facts.get(pattern)
    }

    pub fn expect_capture_fact(&self, pattern: &Pattern) -> &CaptureFact {
        self.capture_fact(pattern)
            .expect("admitted regular capture must have frozen capture facts")
    }

    pub fn field_completions(&self, pattern: &Pattern) -> Option<&FieldCompletions> {
        self.field_completions.get(pattern)
    }

    pub fn expect_field_completions(&self, pattern: &Pattern) -> &FieldCompletions {
        self.field_completions(pattern)
            .expect("every field-producing alternation must have explicit field completions")
    }

    pub fn root_extent(&self, pattern: &Pattern) -> Option<RootExtent> {
        self.pattern_result
            .get(pattern)
            .map(|info| info.root_extent)
    }

    pub fn def_output(&self, def_id: DefId) -> Option<TypeId> {
        self.def_output.get(&def_id).copied()
    }

    pub fn expect_def_output(&self, def_id: DefId) -> TypeId {
        self.def_output(def_id)
            .expect("admitted definition must have an inferred output type")
    }

    pub fn def_root_extent(&self, def_id: DefId) -> Option<RootExtent> {
        self.def_root_extent.get(&def_id).copied()
    }

    pub fn expect_def_root_extent(&self, def_id: DefId) -> RootExtent {
        self.def_root_extent(def_id)
            .expect("admitted definition must have an inferred root extent")
    }

    pub fn is_selectable_definition(&self, def_id: DefId) -> bool {
        self.expect_def_root_extent(def_id) == RootExtent::SingleNode
    }

    /// Follow a `Ref` chain to the underlying materialized type; non-ref types
    /// resolve to themselves. The accessor type-table emission uses to map a
    /// query type to the concrete shape it stands for.
    ///
    /// A `Ref` whose definition ended up void resolves to `Node`: the runtime
    /// capture of such a reference takes the matched node (the callee leaves no
    /// pending value), so `Node` is the shape the reference stands for.
    pub fn resolve_underlying_type_id(&self, type_id: TypeId) -> TypeId {
        let Some(TypeShape::Ref(def_id)) = self.type_shape(type_id) else {
            return type_id;
        };
        let target = self.expect_def_output(*def_id);
        if target == TYPE_VOID {
            return TYPE_NODE;
        }
        self.resolve_underlying_type_id(target)
    }

    /// Iterate over all definition output types as `(DefId, TypeId)` in `DefId`
    /// order, which corresponds to SCC processing order (leaves first).
    pub fn iter_def_output(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        self.def_output.iter().map(|(&id, &type_id)| (id, type_id))
    }

    /// Iterate over selectable definition outputs in definition order.
    pub fn iter_entry_point_outputs(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        self.iter_def_output()
            .filter(|&(def_id, _)| self.is_selectable_definition(def_id))
    }

    /// Iterate generated and explicitly named structural bodies in `TypeId`
    /// order. Definition declarations are exposed separately through their
    /// `DefId` and output body.
    pub fn iter_named_types(&self) -> impl Iterator<Item = (TypeId, Symbol)> + '_ {
        self.named_types.iter().map(|(&id, &sym)| (id, sym))
    }

    /// Admission check for [`TypeAnalysisBuilder::finish`]: the frozen result must
    /// be internally consistent before any trusting accessor reads it. Every
    /// failure here is a type-inference bug, not a query condition, so we assert
    /// loudly — the same discipline `DependencyAnalysis::new` follows.
    fn assert_well_formed(&self) {
        assert!(
            matches!(self.type_shape(TYPE_VOID), Some(TypeShape::Void)),
            "TYPE_VOID must be interned at its canonical id",
        );
        assert!(
            matches!(self.type_shape(TYPE_NODE), Some(TypeShape::Node)),
            "TYPE_NODE must be interned at its canonical id",
        );
        assert!(
            matches!(self.type_shape(TYPE_TEXT), Some(TypeShape::Text)),
            "TYPE_TEXT must be interned at its canonical id",
        );
        assert!(
            matches!(self.type_shape(TYPE_BOOL), Some(TypeShape::Bool)),
            "TYPE_BOOL must be interned at its canonical id",
        );

        for shape in &self.types {
            for child_id in shape.child_type_ids() {
                self.assert_type_id_registered(child_id, "child type id out of range");
            }

            if let TypeShape::Option(inner) = shape {
                assert!(!self.type_is_option(*inner), "Option must be idempotent",);
            }

            if let TypeShape::Ref(def_id) = shape {
                assert!(
                    self.def_output.contains_key(def_id),
                    "every Ref target must have an inferred output type",
                );
            }
        }

        for &type_id in self.def_output.values() {
            self.assert_type_id_registered(type_id, "def output type id out of range");
        }

        assert_eq!(
            self.def_output.len(),
            self.def_root_extent.len(),
            "definition output and root-extent tables must cover the same definitions",
        );
        for def_id in self.def_output.keys() {
            assert!(
                self.def_root_extent.contains_key(def_id),
                "every definition output must have an inferred root extent",
            );
        }
        for def_id in self.def_root_extent.keys() {
            assert!(
                self.def_output.contains_key(def_id),
                "every definition root extent must have an inferred output",
            );
        }

        for info in self.pattern_result.values() {
            self.assert_flow_well_formed(&info.flow);
        }

        let field_alternations = self
            .pattern_result
            .iter()
            .filter(|(pattern, shape)| {
                matches!(pattern, Pattern::Alternation(_))
                    && matches!(&shape.flow, PatternFlow::Fields(_))
            })
            .count();
        assert_eq!(
            self.field_completions.len(),
            field_alternations,
            "field-completion tables must cover exactly the field-producing alternations",
        );
        for (pattern, completions) in &self.field_completions {
            assert!(
                matches!(pattern, Pattern::Alternation(_)),
                "field completions must belong to an alternation",
            );
            let PatternFlow::Fields(type_id) = &self
                .pattern_result
                .get(pattern)
                .expect("field completions must belong to an admitted pattern")
                .flow
            else {
                panic!("field completions must belong to a field-producing alternation")
            };
            let fields = self.expect_record_fields(*type_id);
            assert_eq!(
                completions.fields().count(),
                fields.len(),
                "every merged field must have exactly one completion",
            );
            for field in completions.fields() {
                assert!(
                    fields.contains_key(&field),
                    "field completions cannot name a field outside the merged record",
                );
            }
        }

        for &type_id in self.named_types.keys() {
            self.assert_type_id_registered(type_id, "named type id out of range");
        }
    }

    fn assert_flow_well_formed(&self, flow: &PatternFlow) {
        match flow {
            PatternFlow::Void => {}
            PatternFlow::Value(type_id) => {
                self.assert_type_id_registered(*type_id, "value flow type id out of range");
            }
            PatternFlow::Fields(type_id) => {
                self.assert_type_id_registered(*type_id, "fields flow type id out of range");
                assert!(
                    matches!(self.type_shape(*type_id), Some(TypeShape::Record(_))),
                    "Fields flow must point to a Record type",
                );
            }
        }
    }

    fn assert_type_id_registered(&self, type_id: TypeId, message: &str) {
        assert!(self.type_shape(type_id).is_some(), "{message}");
    }
}

/// Mutable accumulator that produces a [`TypeAnalysis`].
///
/// Owns the in-progress artifact plus the scratch state inference needs but the
/// frozen result does not: the intern-dedup index and the per-definition result
/// memo. [`finish`](Self::finish) drops the scratch and hands back the frozen
/// [`TypeAnalysis`].
pub struct TypeAnalysisBuilder {
    pub(super) analysis: TypeAnalysis,

    /// Reverse index for `intern_type` deduplication of leaf and wrapper shapes.
    /// Records and variant types are deliberately NOT deduplicated: they are nominal —
    /// two definitions with identical capture profiles are two distinct types,
    /// each carrying its own name. Scratch: the frozen result looks types up by
    /// `TypeId`, never by shape.
    intern_index: HashMap<TypeShape, TypeId>,

    /// Creation site of every fresh record/variant type, for naming-pass diagnostics.
    /// Scratch: only the naming pass consults it.
    type_provenance: HashMap<TypeId, Span>,

    /// Custom `:: TypeName` capture-type occurrences in source order.
    /// Scratch: only the naming pass consults it.
    custom_capture_types: Vec<CustomCaptureTypeOccurrence>,

    /// Present only when the builtin pre-scan requests producer provenance.
    /// Keeping the recorder on the builder prevents normalization details from
    /// leaking into the ordinary public type model.
    raw_output_graph: Option<RawOutputGraphBuilder>,

    /// Raw naming failures gate capture-type normalization but have no meaning
    /// after the builder freezes the final public graph.
    pub(super) invalid_types: HashSet<TypeId>,
}

pub(crate) struct TypeAnalysisView<'a> {
    pub(super) analysis: &'a TypeAnalysis,
}

impl TypeAnalysisView<'_> {
    pub(crate) fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.analysis.type_shape(id)
    }

    pub(crate) fn expect_record_fields(&self, id: TypeId) -> &BTreeMap<Symbol, RecordField> {
        self.analysis.expect_record_fields(id)
    }

    pub(crate) fn pattern_result(&self, pattern: &Pattern) -> Option<&PatternShape> {
        self.analysis.pattern_result(pattern)
    }

    pub(crate) fn def_output(&self, def_id: DefId) -> Option<TypeId> {
        self.analysis.def_output(def_id)
    }
}

impl Default for TypeAnalysisBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeAnalysisBuilder {
    pub fn new() -> Self {
        let mut builder = Self {
            analysis: TypeAnalysis {
                types: Vec::new(),
                def_output: BTreeMap::new(),
                def_root_extent: BTreeMap::new(),
                pattern_result: HashMap::new(),
                capture_facts: HashMap::new(),
                field_completions: HashMap::new(),
                named_types: BTreeMap::new(),
            },
            intern_index: HashMap::new(),
            type_provenance: HashMap::new(),
            custom_capture_types: Vec::new(),
            raw_output_graph: None,
            invalid_types: HashSet::new(),
        };

        // Pre-register builtin types at their expected IDs.
        let void_id = builder.intern_type(TypeShape::Void);
        debug_assert_eq!(void_id, TYPE_VOID);

        let node_id = builder.intern_type(TypeShape::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        let text_id = builder.intern_type(TypeShape::Text);
        debug_assert_eq!(text_id, TYPE_TEXT);

        let bool_id = builder.intern_type(TypeShape::Bool);
        debug_assert_eq!(bool_id, TYPE_BOOL);

        builder
    }

    pub(crate) fn for_capture_normalization() -> Self {
        let mut builder = Self::new();
        builder.raw_output_graph = Some(RawOutputGraphBuilder::default());
        builder
    }

    /// Freeze the accumulated state, dropping the inference-only scratch. Admits
    /// the result only after asserting it is internally consistent.
    pub fn finish(self) -> TypeAnalysis {
        assert!(
            self.raw_output_graph.is_none(),
            "capture-type normalization must consume its producer provenance",
        );
        self.analysis.assert_well_formed();
        self.analysis
    }

    /// Restricted read-only view of the in-progress artifact. It exposes only
    /// accessors that are explicitly safe before [`finish`](Self::finish).
    pub(crate) fn in_progress(&self) -> TypeAnalysisView<'_> {
        TypeAnalysisView {
            analysis: &self.analysis,
        }
    }

    /// Intern a type shape. Leaf and wrapper shapes deduplicate structurally;
    /// records and variant types always mint a fresh id (they are nominal — see the
    /// `intern_index` field docs).
    pub fn intern_type(&mut self, shape: TypeShape) -> TypeId {
        if let TypeShape::Option(inner) = &shape
            && self.type_is_option(*inner)
        {
            return *inner;
        }

        if matches!(shape, TypeShape::Record(_) | TypeShape::Variant(_)) {
            let id = TypeId(self.analysis.types.len() as u32);
            self.analysis.types.push(shape);
            return id;
        }

        if let Some(&id) = self.intern_index.get(&shape) {
            return id;
        }

        let id = TypeId(self.analysis.types.len() as u32);
        self.analysis.types.push(shape.clone());
        self.intern_index.insert(shape, id);
        id
    }

    fn type_is_option(&self, type_id: TypeId) -> bool {
        self.analysis.type_is_option(type_id)
    }

    pub fn intern_option(&mut self, inner: TypeId) -> TypeId {
        self.intern_type(TypeShape::Option(inner))
    }

    pub fn intern_record(&mut self, fields: BTreeMap<Symbol, RecordField>) -> TypeId {
        self.intern_type(TypeShape::Record(fields))
    }

    pub fn intern_single_field_record(&mut self, name: Symbol, info: RecordField) -> TypeId {
        self.intern_type(TypeShape::Record(BTreeMap::from([(name, info)])))
    }

    pub fn intern_custom(&mut self, name: Symbol) -> TypeId {
        self.intern_type(TypeShape::Custom(name))
    }

    /// Record where a fresh record/variant type came from, for naming-pass diagnostics.
    pub fn record_type_provenance(&mut self, type_id: TypeId, span: Span) {
        self.type_provenance.entry(type_id).or_insert(span);
    }

    pub fn type_provenance(&self, type_id: TypeId) -> Option<Span> {
        self.type_provenance.get(&type_id).copied()
    }

    /// Record a custom `:: TypeName` occurrence for the naming pass.
    pub fn record_custom_capture_type(&mut self, occurrence: CustomCaptureTypeOccurrence) {
        self.custom_capture_types.push(occurrence);
    }

    pub fn custom_capture_types(&self) -> &[CustomCaptureTypeOccurrence] {
        &self.custom_capture_types
    }

    pub fn record_pattern_result(
        &mut self,
        pattern: Pattern,
        source: crate::compiler::diagnostics::source::SourceId,
        shape: PatternShape,
    ) {
        if let Some(graph) = &mut self.raw_output_graph {
            graph.record_pattern(pattern.clone(), source, &shape, &self.analysis);
        }
        self.analysis.pattern_result.insert(pattern, shape);
    }

    pub(crate) fn record_raw_capture_observation(
        &mut self,
        pattern: Pattern,
        observation: RawCaptureObservation,
    ) {
        let Some(graph) = &mut self.raw_output_graph else {
            return;
        };
        graph.record_capture(pattern, observation);
    }

    pub(crate) fn records_raw_output_provenance(&self) -> bool {
        self.raw_output_graph.is_some()
    }

    pub fn record_capture_fact(&mut self, pattern: Pattern, fact: CaptureFact) {
        self.analysis.capture_facts.insert(pattern, fact);
    }

    pub fn record_field_completions(&mut self, pattern: Pattern, completions: FieldCompletions) {
        self.analysis.field_completions.insert(pattern, completions);
    }

    pub(crate) fn record_invalid_type(&mut self, type_id: TypeId) {
        self.invalid_types.insert(type_id);
    }

    pub(crate) fn record_alternation_incompatibility(&mut self, pattern: Pattern, field: Symbol) {
        let Some(graph) = &mut self.raw_output_graph else {
            return;
        };
        graph.record_alternation_incompatibility(pattern, field);
    }

    /// Snapshot the alternation outputs that need field-completion tables.
    /// The snapshot breaks the immutable borrow before tables are inserted
    /// without cloning every inferred pattern and shape in the query.
    pub(crate) fn alternation_field_results(&self) -> Vec<(Pattern, TypeId)> {
        self.analysis
            .pattern_result
            .iter()
            .filter_map(|(pattern, shape)| {
                if !matches!(pattern, Pattern::Alternation(_)) {
                    return None;
                }
                let PatternFlow::Fields(type_id) = shape.flow else {
                    return None;
                };
                Some((pattern.clone(), type_id))
            })
            .collect()
    }

    pub fn record_def_output(&mut self, def_id: DefId, type_id: TypeId) {
        self.analysis.def_output.insert(def_id, type_id);
    }

    pub(crate) fn record_raw_definition(
        &mut self,
        def_id: DefId,
        body: &Pattern,
        value_role: RawDefinitionValueRole,
    ) {
        let Some(graph) = &mut self.raw_output_graph else {
            return;
        };
        graph.record_definition(def_id, body, value_role);
    }

    pub(crate) fn normalize_capture_types(
        &mut self,
        interner: &crate::core::Interner,
        diagnostics: &mut Diagnostics,
    ) {
        let graph = self
            .raw_output_graph
            .take()
            .expect("capture-type pre-scan enables producer provenance");
        graph.finish().normalize(self, interner, diagnostics);
    }

    pub fn record_def_root_extent(&mut self, def_id: DefId, extent: RootExtent) {
        self.analysis.def_root_extent.insert(def_id, extent);
    }

    pub fn def_root_extent(&self, def_id: DefId) -> Option<RootExtent> {
        self.analysis.def_root_extent(def_id)
    }

    /// Install the naming pass's result. Names must be complete and validated
    /// before the analysis is frozen.
    pub fn set_named_types(&mut self, names: BTreeMap<TypeId, Symbol>) {
        self.analysis.named_types = names;
    }

    /// Deep structural equality over the in-progress type registry.
    ///
    /// Record and variant bodies mint a fresh id per occurrence, so two
    /// structurally identical bodies can carry different ids. References to
    /// declarations with record or variant bodies remain nominal; transparent
    /// aliases compare through their bodies. `Ref` cuts recursion, so the walk
    /// terminates on recursive types.
    pub(crate) fn types_structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        let a = self.transparent_alias_body(a);
        let b = self.transparent_alias_body(b);
        if a == b {
            return true;
        }

        let (Some(shape_a), Some(shape_b)) =
            (self.analysis.type_shape(a), self.analysis.type_shape(b))
        else {
            return false;
        };

        match (shape_a, shape_b) {
            (TypeShape::Record(fa), TypeShape::Record(fb)) => {
                fa.len() == fb.len()
                    && fa.iter().zip(fb.iter()).all(|((ka, ia), (kb, ib))| {
                        ka == kb && self.types_structurally_equal(ia.final_type, ib.final_type)
                    })
            }
            (TypeShape::Variant(va), TypeShape::Variant(vb)) => {
                va.len() == vb.len()
                    && va.iter().zip(vb.iter()).all(|((ka, pa), (kb, pb))| {
                        ka == kb && self.types_structurally_equal(*pa, *pb)
                    })
            }
            (
                TypeShape::Array {
                    element: ea,
                    non_empty: na,
                },
                TypeShape::Array {
                    element: eb,
                    non_empty: nb,
                },
            ) => na == nb && self.types_structurally_equal(*ea, *eb),
            (TypeShape::Option(ia), TypeShape::Option(ib)) => {
                self.types_structurally_equal(*ia, *ib)
            }
            _ => false,
        }
    }

    fn transparent_alias_body(&self, mut type_id: TypeId) -> TypeId {
        let mut seen = HashSet::new();
        while let Some(TypeShape::Ref(def_id)) = self.analysis.type_shape(type_id) {
            if !seen.insert(*def_id) {
                return type_id;
            }
            let Some(body) = self.analysis.def_output(*def_id) else {
                return type_id;
            };
            match self.analysis.type_shape(body) {
                Some(TypeShape::Record(_) | TypeShape::Variant(_)) => return type_id,
                Some(TypeShape::Ref(_)) => type_id = body,
                Some(_) => return body,
                None => return type_id,
            }
        }
        type_id
    }
}
