//! `TypeAnalysis`: the frozen result of type inference.
//!
//! Holds the interned type registry, named type declarations, each definition's
//! output and root extent, and the per-pattern inference results. It is built
//! incrementally by [`TypeAnalysisBuilder`] and frozen with
//! [`TypeAnalysisBuilder::finish`]; past that boundary it is immutable and its
//! accessors are trusted (a structural miss is a compiler bug, not a query
//! condition).
//!
//! Definition matching identity remains owned by `DependencyAnalysis`. This
//! artifact records the separate type declaration owned by each definition.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compiler::analyze::Located;
use crate::compiler::analyze::types::capture::{CaptureId, CaptureObservation, CaptureProvenance};
use crate::compiler::analyze::types::type_shape::{
    DefinitionOutput, PatternFlow, PatternShape, RESERVED_NO_VALUE_TYPE_ID, RecordField, TYPE_BOOL,
    TYPE_NODE, TYPE_TEXT, TypeId, TypeShape,
};
use crate::compiler::analyze::types::{CaptureFact, FieldCompletions, RootExtent};
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::ids::{DefId, TypeDeclId};
use crate::compiler::parse::ast::{CapturedPattern, Pattern};
use crate::core::Symbol;

/// One custom `:: TypeName` occurrence, recorded during inference for the
/// naming pass to validate (nominal identity, collisions, redundancy).
#[derive(Clone, Copy, Debug)]
pub struct CustomCaptureTypeOccurrence {
    pub name: Symbol,
    pub span: Span,
    pub type_id: TypeId,
}

/// A named result type whose identity is independent of its structural body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeDeclaration {
    pub id: TypeDeclId,
    pub name: Symbol,
    pub body: TypeId,
}

#[derive(Clone, Debug)]
struct TypeDeclarationEntry {
    name: Symbol,
    body: Option<TypeId>,
    definition: Option<DefId>,
}

/// Frozen registry of inferred types and per-definition / per-pattern results.
///
/// Constructed only via [`TypeAnalysisBuilder`]; the private fields and
/// `#[non_exhaustive]` keep that the single entry point.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TypeAnalysis {
    types: Vec<TypeEntry>,
    declarations: Vec<TypeDeclarationEntry>,
    definition_declarations: BTreeMap<DefId, TypeDeclId>,

    /// Each definition's output, keyed by `DefId`. `BTreeMap` so iteration
    /// is in `DefId` order — the SCC/emission order entry points rely on. Total
    /// over every scheduled definition, including match-only definitions.
    pub(super) def_output: BTreeMap<DefId, DefinitionOutput>,

    /// Each definition's static top-level extent.
    def_root_extent: BTreeMap<DefId, RootExtent>,

    /// Whether the definition exports a leading or trailing anchor obligation.
    /// Such definitions remain reusable fragments even when they consume
    /// exactly one root node: a contextless entry point cannot discharge them.
    def_requires_anchor_context: BTreeMap<DefId, bool>,

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

#[derive(Clone, Debug)]
enum TypeEntry {
    ReservedNoValue,
    Shape(TypeShape),
}

impl TypeAnalysis {
    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        match self.types.get(id.0 as usize)? {
            TypeEntry::ReservedNoValue => None,
            TypeEntry::Shape(shape) => Some(shape),
        }
    }

    pub fn expect_type_shape(&self, id: TypeId) -> &TypeShape {
        self.type_shape(id)
            .expect("admitted type id must reference a registered type")
    }

    /// Deep structural equality over the frozen type registry.
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

        let (Some(shape_a), Some(shape_b)) = (self.type_shape(a), self.type_shape(b)) else {
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
                        ka == kb
                            && match (pa.type_id(), pb.type_id()) {
                                (None, None) => true,
                                (Some(a), Some(b)) => self.types_structurally_equal(a, b),
                                _ => false,
                            }
                    })
            }
            (
                TypeShape::List {
                    element: ea,
                    minimum: ma,
                },
                TypeShape::List {
                    element: eb,
                    minimum: mb,
                },
            ) => ma == mb && self.types_structurally_equal(*ea, *eb),
            (TypeShape::Option(ia), TypeShape::Option(ib)) => {
                self.types_structurally_equal(*ia, *ib)
            }
            _ => false,
        }
    }

    fn transparent_alias_body(&self, mut type_id: TypeId) -> TypeId {
        let mut seen = HashSet::new();
        while let Some(TypeShape::Ref(declaration)) = self.type_shape(type_id) {
            if !seen.insert(*declaration) {
                return type_id;
            }
            let Some(body) = self.declaration_body(*declaration) else {
                return type_id;
            };
            match self.type_shape(body) {
                Some(TypeShape::Record(_) | TypeShape::Variant(_)) => return type_id,
                Some(TypeShape::Ref(_)) => type_id = body,
                Some(_) => return body,
                None => return type_id,
            }
        }
        type_id
    }

    pub fn declaration(&self, id: TypeDeclId) -> Option<TypeDeclaration> {
        let entry = self.declarations.get(id.index())?;
        Some(TypeDeclaration {
            id,
            name: entry.name,
            body: entry.body?,
        })
    }

    pub fn declaration_body(&self, id: TypeDeclId) -> Option<TypeId> {
        self.declarations
            .get(id.index())
            .and_then(|entry| entry.body)
    }

    pub fn declaration_name(&self, id: TypeDeclId) -> Symbol {
        self.declarations
            .get(id.index())
            .expect("type declaration id must be registered")
            .name
    }

    pub fn declaration_definition(&self, id: TypeDeclId) -> Option<DefId> {
        self.declarations
            .get(id.index())
            .expect("type declaration id must be registered")
            .definition
    }

    pub fn definition_declaration(&self, def_id: DefId) -> TypeDeclId {
        *self
            .definition_declarations
            .get(&def_id)
            .expect("every definition must own a type declaration slot")
    }

    fn type_is_option(&self, mut type_id: TypeId) -> bool {
        let mut seen = HashSet::new();
        while seen.insert(type_id) {
            match self.type_shape(type_id) {
                Some(TypeShape::Option(_)) => return true,
                Some(TypeShape::Ref(declaration)) => {
                    let Some(target) = self.declaration_body(*declaration) else {
                        return false;
                    };
                    type_id = target;
                }
                _ => return false,
            }
        }
        false
    }

    /// Fields of the record a `Fields` flow points to.
    ///
    /// Every `PatternFlow::Fields` is constructed by interning a `Record`, so a
    /// non-`Record` id here is a broken type-system invariant, not a runtime
    /// condition the query can trigger. We surface it loudly instead of fabricating
    /// an empty record that would silently mistype the output.
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
    /// A `Ref` resolves through its target: a reference to a match-only definition
    /// leaves no pending value at runtime (the capture takes the matched node),
    /// so it must not classify as structured. Mid-inference a same-SCC target
    /// has no inferred output flow yet; assume structured — the admitted classification that
    /// lowering reads always resolves.
    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        match self.type_shape(type_id) {
            Some(TypeShape::Variant(_) | TypeShape::Record(_)) => true,
            Some(TypeShape::Ref(declaration)) => self
                .declaration_body(*declaration)
                .is_none_or(|t| self.is_structured_output(t)),
            Some(shape @ (TypeShape::List { .. } | TypeShape::Option(_))) => shape
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

    pub fn def_output(&self, def_id: DefId) -> Option<DefinitionOutput> {
        self.def_output.get(&def_id).copied()
    }

    pub fn expect_def_output(&self, def_id: DefId) -> DefinitionOutput {
        self.def_output(def_id)
            .expect("admitted definition must have an inferred output")
    }

    pub fn def_root_extent(&self, def_id: DefId) -> Option<RootExtent> {
        self.def_root_extent.get(&def_id).copied()
    }

    pub fn expect_def_root_extent(&self, def_id: DefId) -> RootExtent {
        self.def_root_extent(def_id)
            .expect("admitted definition must have an inferred root extent")
    }

    pub fn def_requires_anchor_context(&self, def_id: DefId) -> bool {
        *self
            .def_requires_anchor_context
            .get(&def_id)
            .expect("admitted definition must have an anchor-context classification")
    }

    pub fn is_selectable_definition(&self, def_id: DefId) -> bool {
        self.expect_def_root_extent(def_id) == RootExtent::SingleNode
            && !self.def_requires_anchor_context(def_id)
    }

    /// Follow a `Ref` chain to the underlying materialized type; non-ref types
    /// resolve to themselves. The accessor type-table emission uses to map a
    /// query type to the concrete shape it stands for.
    ///
    /// A `Ref` whose definition is match-only resolves to `Node`: the runtime
    /// capture of such a reference takes the matched node (the callee leaves no
    /// pending value), so `Node` is the shape the reference stands for.
    pub fn resolve_underlying_type_id(&self, type_id: TypeId) -> TypeId {
        let Some(TypeShape::Ref(declaration)) = self.type_shape(type_id) else {
            return type_id;
        };
        let Some(target) = self.declaration_body(*declaration) else {
            return TYPE_NODE;
        };
        self.resolve_underlying_type_id(target)
    }

    /// Iterate over all definition result types as `(DefId, TypeId)` in `DefId`
    /// order, which corresponds to SCC processing order (leaves first).
    pub fn iter_def_output(&self) -> impl Iterator<Item = (DefId, DefinitionOutput)> + '_ {
        self.def_output.iter().map(|(&id, &output)| (id, output))
    }

    /// Iterate over selectable definition outputs in definition order.
    pub fn iter_entry_point_outputs(&self) -> impl Iterator<Item = (DefId, DefinitionOutput)> + '_ {
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
            matches!(
                self.types.get(RESERVED_NO_VALUE_TYPE_ID.0 as usize),
                Some(TypeEntry::ReservedNoValue)
            ),
            "the bytecode no-value slot must remain reserved",
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

        for entry in &self.types {
            let TypeEntry::Shape(shape) = entry else {
                continue;
            };
            for child_id in shape.child_type_ids() {
                self.assert_type_id_registered(child_id, "child type id out of range");
            }

            if let TypeShape::Option(inner) = shape {
                assert!(!self.type_is_option(*inner), "Option must be idempotent",);
            }

            if let TypeShape::Ref(declaration) = shape {
                assert!(
                    self.declarations.get(declaration.index()).is_some(),
                    "every Ref target must be a registered type declaration",
                );
            }
        }

        for output in self.def_output.values() {
            if let DefinitionOutput::Value(type_id) = output {
                self.assert_type_id_registered(*type_id, "definition result type id out of range");
            }
        }

        for (&def_id, &declaration) in &self.definition_declarations {
            let entry = self
                .declarations
                .get(declaration.index())
                .expect("definition declaration id must be registered");
            assert_eq!(entry.definition, Some(def_id));
            assert_eq!(entry.body, self.expect_def_output(def_id).value());
        }

        assert_eq!(
            self.def_output.len(),
            self.def_root_extent.len(),
            "definition output and root-extent tables must cover the same definitions",
        );
        assert_eq!(
            self.def_output.len(),
            self.def_requires_anchor_context.len(),
            "definition output and anchor-context tables must cover the same definitions",
        );
        for def_id in self.def_output.keys() {
            assert!(
                self.def_root_extent.contains_key(def_id),
                "every definition output must have an inferred root extent",
            );
            assert!(
                self.def_requires_anchor_context.contains_key(def_id),
                "every definition output must have an anchor-context classification",
            );
        }
        for def_id in self.def_root_extent.keys() {
            assert!(
                self.def_output.contains_key(def_id),
                "every definition root extent must have an inferred output",
            );
        }
        for def_id in self.def_requires_anchor_context.keys() {
            assert!(
                self.def_output.contains_key(def_id),
                "every anchor-context classification must have an inferred output",
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
            PatternFlow::NoValue => {}
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

    /// Explicit capture-type declarations deduplicated by name and body.
    capture_type_declarations: HashMap<(Symbol, TypeId), TypeDeclId>,

    capture_provenance: CaptureProvenance,
    pattern_order: Vec<Pattern>,

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

    pub(crate) fn def_output(&self, def_id: DefId) -> Option<DefinitionOutput> {
        self.analysis.def_output(def_id)
    }

    pub(crate) fn declaration_body(&self, declaration: TypeDeclId) -> Option<TypeId> {
        self.analysis.declaration_body(declaration)
    }

    pub(crate) fn declaration_definition(&self, declaration: TypeDeclId) -> Option<DefId> {
        self.analysis.declaration_definition(declaration)
    }

    pub(crate) fn declaration_name(&self, declaration: TypeDeclId) -> Symbol {
        self.analysis.declaration_name(declaration)
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
                declarations: Vec::new(),
                definition_declarations: BTreeMap::new(),
                def_output: BTreeMap::new(),
                def_root_extent: BTreeMap::new(),
                def_requires_anchor_context: BTreeMap::new(),
                pattern_result: HashMap::new(),
                capture_facts: HashMap::new(),
                field_completions: HashMap::new(),
                named_types: BTreeMap::new(),
            },
            intern_index: HashMap::new(),
            type_provenance: HashMap::new(),
            custom_capture_types: Vec::new(),
            capture_type_declarations: HashMap::new(),
            capture_provenance: CaptureProvenance::default(),
            pattern_order: Vec::new(),
            invalid_types: HashSet::new(),
        };

        // Preserve bytecode primitive numbering without treating no-value flow as a type.
        builder.analysis.types.push(TypeEntry::ReservedNoValue);
        debug_assert_eq!(builder.analysis.types.len(), TYPE_NODE.0 as usize);

        let node_id = builder.intern_type(TypeShape::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        let text_id = builder.intern_type(TypeShape::Text);
        debug_assert_eq!(text_id, TYPE_TEXT);

        let bool_id = builder.intern_type(TypeShape::Bool);
        debug_assert_eq!(bool_id, TYPE_BOOL);

        builder
    }

    /// Freeze the accumulated state, dropping the inference-only scratch. Admits
    /// the result only after asserting it is internally consistent.
    pub fn finish(self) -> TypeAnalysis {
        assert!(
            self.capture_provenance.captures.is_empty(),
            "capture-type normalization must consume capture provenance",
        );
        assert!(
            self.pattern_order.is_empty(),
            "capture-type normalization must consume inference order",
        );
        assert!(
            self.analysis
                .pattern_result
                .values()
                .all(|shape| shape.field_flow.is_none()),
            "capture-type normalization must consume field provenance",
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
            self.analysis.types.push(TypeEntry::Shape(shape));
            return id;
        }

        if let Some(&id) = self.intern_index.get(&shape) {
            return id;
        }

        let id = TypeId(self.analysis.types.len() as u32);
        self.analysis.types.push(TypeEntry::Shape(shape.clone()));
        self.intern_index.insert(shape, id);
        id
    }

    pub(super) fn type_shapes_snapshot(&self) -> Vec<Option<TypeShape>> {
        self.analysis
            .types
            .iter()
            .map(|entry| match entry {
                TypeEntry::ReservedNoValue => None,
                TypeEntry::Shape(shape) => Some(shape.clone()),
            })
            .collect()
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

    pub(super) fn replace_record_fields(
        &mut self,
        type_id: TypeId,
        fields: BTreeMap<Symbol, RecordField>,
    ) {
        let Some(TypeEntry::Shape(TypeShape::Record(current))) =
            self.analysis.types.get_mut(type_id.0 as usize)
        else {
            unreachable!("record field replacement requires a registered record")
        };
        *current = fields;
    }

    pub fn declare_definitions(&mut self, definitions: impl IntoIterator<Item = (DefId, Symbol)>) {
        for (def_id, name) in definitions {
            assert!(
                !self.analysis.definition_declarations.contains_key(&def_id),
                "definition declaration slots are reserved once",
            );
            let id = TypeDeclId::from_raw(
                u32::try_from(self.analysis.declarations.len())
                    .expect("type declaration count fits u32"),
            );
            self.analysis.declarations.push(TypeDeclarationEntry {
                name,
                body: None,
                definition: Some(def_id),
            });
            self.analysis.definition_declarations.insert(def_id, id);
        }
    }

    pub fn definition_ref(&mut self, def_id: DefId) -> TypeId {
        let declaration = self.analysis.definition_declaration(def_id);
        self.intern_type(TypeShape::Ref(declaration))
    }

    pub fn declare_capture_type(&mut self, name: Symbol, body: TypeId) -> TypeId {
        if let Some(&declaration) = self.capture_type_declarations.get(&(name, body)) {
            return self.intern_type(TypeShape::Ref(declaration));
        }
        let declaration = TypeDeclId::from_raw(
            u32::try_from(self.analysis.declarations.len())
                .expect("type declaration count fits u32"),
        );
        self.analysis.declarations.push(TypeDeclarationEntry {
            name,
            body: Some(body),
            definition: None,
        });
        self.capture_type_declarations
            .insert((name, body), declaration);
        self.intern_type(TypeShape::Ref(declaration))
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

    pub fn record_pattern_result(&mut self, pattern: Pattern, shape: PatternShape) {
        if !self.analysis.pattern_result.contains_key(&pattern) {
            self.pattern_order.push(pattern.clone());
        }
        self.analysis.pattern_result.insert(pattern, shape);
    }

    pub(super) fn record_capture(
        &mut self,
        captured_pattern: Located<CapturedPattern>,
        observation: CaptureObservation,
    ) -> CaptureId {
        self.capture_provenance
            .record_capture(captured_pattern, observation)
    }

    pub fn record_capture_fact(&mut self, pattern: Pattern, fact: CaptureFact) {
        self.analysis.capture_facts.insert(pattern, fact);
    }

    pub(crate) fn record_invalid_type(&mut self, type_id: TypeId) {
        self.invalid_types.insert(type_id);
    }

    pub(super) fn block_capture_producers(
        &mut self,
        producers: impl IntoIterator<Item = CaptureId>,
    ) {
        self.capture_provenance.block_captures(producers);
    }

    pub(crate) fn has_built_in_capture_types(&self) -> bool {
        self.capture_provenance.has_built_in_capture_types()
    }

    pub fn record_def_output(&mut self, def_id: DefId, output: DefinitionOutput) {
        self.analysis.def_output.insert(def_id, output);
        let declaration = self.analysis.definition_declaration(def_id);
        self.analysis.declarations[declaration.index()].body = output.value();
    }

    pub(crate) fn normalize_capture_types(
        &mut self,
        interner: &crate::core::Interner,
        diagnostics: &mut Diagnostics,
    ) {
        let provenance = std::mem::take(&mut self.capture_provenance);
        let pattern_order = std::mem::take(&mut self.pattern_order);
        provenance.normalize(pattern_order, self, interner, diagnostics);
    }

    pub fn record_def_root_extent(&mut self, def_id: DefId, extent: RootExtent) {
        self.analysis.def_root_extent.insert(def_id, extent);
    }

    pub fn def_root_extent(&self, def_id: DefId) -> Option<RootExtent> {
        self.analysis.def_root_extent(def_id)
    }

    pub fn record_def_requires_anchor_context(&mut self, def_id: DefId, requires_context: bool) {
        self.analysis
            .def_requires_anchor_context
            .insert(def_id, requires_context);
    }

    /// Install the naming pass's result. Names must be complete and validated
    /// before the analysis is frozen.
    pub fn set_named_types(&mut self, names: BTreeMap<TypeId, Symbol>) {
        self.analysis.named_types = names;
    }

    pub(crate) fn types_structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        self.analysis.types_structurally_equal(a, b)
    }
}
