//! `TypeAnalysis`: the frozen result of type inference.
//!
//! Holds the interned type registry, each definition's output type and arity,
//! the per-pattern inference results, and explicit type aliases. It is built
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
    Arity, FieldInfo, PatternFlow, PatternShape, TYPE_BOOL, TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId,
    TypeShape,
};
use crate::compiler::analyze::types::{CaptureFact, UnionFlowPlan};
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

    /// Each definition's structural arity. `Arity::One` definitions are
    /// callable entrypoints; `Arity::Many` definitions are fragments that can be
    /// referenced or nested but get no top-level entry surface.
    def_arity: BTreeMap<DefId, Arity>,

    pub(super) pattern_result: HashMap<Pattern, PatternShape>,

    /// Raw capture mechanism plus the optional built-in capture-type plan for
    /// every admitted regular capture occurrence.
    pub(super) capture_facts: HashMap<Pattern, CaptureFact>,

    /// Concrete missing-field behavior for each union-like alternation.
    pub(super) union_flow: HashMap<Pattern, UnionFlowPlan>,

    /// Every named type, assigned by the naming pass: definition results carry
    /// their definition's name, nested composites carry path-derived names
    /// (`FooItems`), and custom `:: TypeName` capture types override. Complete: every
    /// struct/variant type reachable from a definition output outside a case
    /// payload position has exactly one name. `BTreeMap` for deterministic
    /// emission order.
    type_names: BTreeMap<TypeId, Symbol>,
}

impl TypeAnalysis {
    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.types.get(id.0 as usize)
    }

    pub fn expect_type_shape(&self, id: TypeId) -> &TypeShape {
        self.type_shape(id)
            .expect("admitted type id must reference a registered type")
    }

    pub fn struct_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, FieldInfo>> {
        match self.type_shape(id)? {
            TypeShape::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Fields of the struct a `Fields` flow points to.
    ///
    /// Every `PatternFlow::Fields` is constructed by interning a `Struct` (see the
    /// `intern_struct`/`intern_single_field` calls at every `Fields` construction
    /// site), so a non-`Struct` id here is a broken type-system invariant, not a
    /// runtime condition the query can trigger. We surface it loudly instead of
    /// fabricating an empty struct that would silently mistype the output.
    pub fn expect_struct_fields(&self, id: TypeId) -> &BTreeMap<Symbol, FieldInfo> {
        match self.expect_type_shape(id) {
            TypeShape::Struct(fields) => fields,
            _ => panic!("Fields flow must point to a Struct type"),
        }
    }

    /// Whether a type is a meaningful structured output (variant/struct, or a
    /// array/optional thereof). Plain `Node` is not — it is the matched node,
    /// captured directly.
    ///
    /// A `Ref` resolves through its target: a reference to a void definition
    /// leaves no pending value at runtime (the capture takes the matched node),
    /// so it must not classify as structured. Mid-inference a same-SCC target
    /// has no output yet; assume structured — the admitted classification that
    /// lowering reads always resolves.
    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        match self.type_shape(type_id) {
            Some(TypeShape::Variant(_) | TypeShape::Struct(_)) => true,
            Some(TypeShape::Ref(def_id)) => self
                .def_output(*def_id)
                .is_none_or(|t| self.is_structured_output(t)),
            Some(shape @ (TypeShape::Array { .. } | TypeShape::Optional(_))) => shape
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

    pub fn union_flow_plan(&self, pattern: &Pattern) -> Option<&UnionFlowPlan> {
        self.union_flow.get(pattern)
    }

    pub fn expect_union_flow_plan(&self, pattern: &Pattern) -> &UnionFlowPlan {
        self.union_flow_plan(pattern)
            .expect("admitted union flow must have an explicit fallback plan")
    }

    pub fn arity(&self, pattern: &Pattern) -> Option<Arity> {
        self.pattern_result.get(pattern).map(|info| info.arity)
    }

    pub fn def_output(&self, def_id: DefId) -> Option<TypeId> {
        self.def_output.get(&def_id).copied()
    }

    pub fn expect_def_output(&self, def_id: DefId) -> TypeId {
        self.def_output(def_id)
            .expect("admitted definition must have an inferred output type")
    }

    pub fn def_arity(&self, def_id: DefId) -> Option<Arity> {
        self.def_arity.get(&def_id).copied()
    }

    pub fn expect_def_arity(&self, def_id: DefId) -> Arity {
        self.def_arity(def_id)
            .expect("admitted definition must have an inferred arity")
    }

    pub fn is_entrypoint_def(&self, def_id: DefId) -> bool {
        self.expect_def_arity(def_id) == Arity::One
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

    /// Iterate over callable definition outputs in definition order.
    pub fn iter_entrypoint_output(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        self.iter_def_output()
            .filter(|&(def_id, _)| self.is_entrypoint_def(def_id))
    }

    /// Iterate all named types in `TypeId` order (deterministic).
    pub fn iter_type_names(&self) -> impl Iterator<Item = (TypeId, Symbol)> + '_ {
        self.type_names.iter().map(|(&id, &sym)| (id, sym))
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
            matches!(self.type_shape(TYPE_STR), Some(TypeShape::Str)),
            "TYPE_STR must be interned at its canonical id",
        );
        assert!(
            matches!(self.type_shape(TYPE_BOOL), Some(TypeShape::Bool)),
            "TYPE_BOOL must be interned at its canonical id",
        );

        for shape in &self.types {
            for child_id in shape.child_type_ids() {
                self.assert_type_id_registered(child_id, "child type id out of range");
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
            self.def_arity.len(),
            "definition output and arity tables must cover the same definitions",
        );
        for def_id in self.def_output.keys() {
            assert!(
                self.def_arity.contains_key(def_id),
                "every definition output must have an inferred arity",
            );
        }
        for def_id in self.def_arity.keys() {
            assert!(
                self.def_output.contains_key(def_id),
                "every definition arity must have an inferred output",
            );
        }

        for info in self.pattern_result.values() {
            self.assert_flow_well_formed(&info.flow);
        }

        for &type_id in self.type_names.keys() {
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
                    matches!(self.type_shape(*type_id), Some(TypeShape::Struct(_))),
                    "Fields flow must point to a Struct type",
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
    /// Structs and variant types are deliberately NOT deduplicated: they are nominal —
    /// two definitions with identical capture profiles are two distinct types,
    /// each carrying its own name. Scratch: the frozen result looks types up by
    /// `TypeId`, never by shape.
    intern_index: HashMap<TypeShape, TypeId>,

    /// Creation site of every fresh struct/variant type, for naming-pass diagnostics.
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

    pub(crate) fn expect_struct_fields(&self, id: TypeId) -> &BTreeMap<Symbol, FieldInfo> {
        self.analysis.expect_struct_fields(id)
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
                def_arity: BTreeMap::new(),
                pattern_result: HashMap::new(),
                capture_facts: HashMap::new(),
                union_flow: HashMap::new(),
                type_names: BTreeMap::new(),
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

        let str_id = builder.intern_type(TypeShape::Str);
        debug_assert_eq!(str_id, TYPE_STR);

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
    /// structs and variant types always mint a fresh id (they are nominal — see the
    /// `intern_index` field docs).
    pub fn intern_type(&mut self, shape: TypeShape) -> TypeId {
        if matches!(shape, TypeShape::Struct(_) | TypeShape::Variant(_)) {
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

    pub fn intern_struct(&mut self, fields: BTreeMap<Symbol, FieldInfo>) -> TypeId {
        self.intern_type(TypeShape::Struct(fields))
    }

    pub fn intern_single_field(&mut self, name: Symbol, info: FieldInfo) -> TypeId {
        self.intern_type(TypeShape::Struct(BTreeMap::from([(name, info)])))
    }

    pub fn intern_custom(&mut self, name: Symbol) -> TypeId {
        self.intern_type(TypeShape::Custom(name))
    }

    /// Record where a fresh struct/variant type came from, for naming-pass diagnostics.
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

    pub fn record_union_flow(&mut self, pattern: Pattern, plan: UnionFlowPlan) {
        self.analysis.union_flow.insert(pattern, plan);
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

    /// Snapshot only the alternation outputs needed to freeze omission plans.
    /// The snapshot breaks the immutable borrow before plans are inserted
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

    pub fn record_def_arity(&mut self, def_id: DefId, arity: Arity) {
        self.analysis.def_arity.insert(def_id, arity);
    }

    pub fn def_arity(&self, def_id: DefId) -> Option<Arity> {
        self.analysis.def_arity(def_id)
    }

    /// Install the naming pass's result. Names must be complete and validated
    /// before the analysis is frozen.
    pub fn set_type_names(&mut self, names: BTreeMap<TypeId, Symbol>) {
        self.analysis.type_names = names;
    }

    /// Deep structural equality over the in-progress type registry.
    ///
    /// Structs and variant types mint a fresh id per occurrence (nominal typing), so
    /// two structurally identical composites can carry different ids; interned
    /// shapes (Node, Custom, Ref, and wrappers over shared ids) compare by id.
    /// `Ref` cuts recursion, so the walk terminates on recursive types.
    pub(crate) fn types_structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }

        let (Some(shape_a), Some(shape_b)) =
            (self.analysis.type_shape(a), self.analysis.type_shape(b))
        else {
            return false;
        };

        match (shape_a, shape_b) {
            (TypeShape::Struct(fa), TypeShape::Struct(fb)) => {
                fa.len() == fb.len()
                    && fa.iter().zip(fb.iter()).all(|((ka, ia), (kb, ib))| {
                        ka == kb
                            && ia.optional == ib.optional
                            && self.types_structurally_equal(ia.type_id, ib.type_id)
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
            (TypeShape::Optional(ia), TypeShape::Optional(ib)) => {
                self.types_structurally_equal(*ia, *ib)
            }
            _ => false,
        }
    }
}
