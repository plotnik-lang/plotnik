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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::compiler::analyze::Located;
use crate::compiler::analyze::types::capture::{
    CaptureId, CaptureObservation, CaptureProvenance, FieldSource,
};
use crate::compiler::analyze::types::type_shape::{
    CasePayload, DefinitionOutput, ListMinimum, PatternFlow, PatternShape,
    RESERVED_NO_VALUE_TYPE_ID, RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TypeId, TypeShape,
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
    name: Option<Symbol>,
    state: DeclarationState,
    owner: DeclarationOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeclarationState {
    Pending,
    MatchOnly,
    Value(TypeId),
}

impl DeclarationState {
    fn from_output(output: DefinitionOutput) -> Self {
        match output {
            DefinitionOutput::MatchOnly => Self::MatchOnly,
            DefinitionOutput::Value(type_id) => Self::Value(type_id),
        }
    }

    fn output(self) -> Option<DefinitionOutput> {
        match self {
            Self::Pending => None,
            Self::MatchOnly => Some(DefinitionOutput::MatchOnly),
            Self::Value(type_id) => Some(DefinitionOutput::Value(type_id)),
        }
    }

    fn value(self) -> Option<TypeId> {
        match self {
            Self::Value(type_id) => Some(type_id),
            Self::Pending | Self::MatchOnly => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeclarationOwner {
    Definition(DefId),
    CaptureType,
    Inference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeRelation {
    Equal,
    Distinct,
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TypeUnification {
    Unified(TypeId),
    Distinct,
    Pending,
}

#[derive(Clone, Debug)]
pub(in crate::compiler::analyze::types) enum UnifyError {
    IncompatibleFieldTypes {
        field: Symbol,
        left_type: TypeId,
        right_type: TypeId,
        name_spans: Vec<(Span, TypeId)>,
        producers: BTreeSet<CaptureId>,
        fallback_span: Span,
    },
}

impl UnifyError {
    pub(in crate::compiler::analyze::types) fn producers(
        &self,
    ) -> impl Iterator<Item = CaptureId> + '_ {
        match self {
            Self::IncompatibleFieldTypes { producers, .. } => producers.iter().copied(),
        }
    }

    pub(in crate::compiler::analyze::types) fn fallback_span(&self) -> Span {
        match self {
            Self::IncompatibleFieldTypes { fallback_span, .. } => *fallback_span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComparisonOperand {
    Type(TypeId),
    Pending,
}

impl TypeRelation {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Distinct, _) | (_, Self::Distinct) => Self::Distinct,
            (Self::Pending, _) | (_, Self::Pending) => Self::Pending,
            (Self::Equal, Self::Equal) => Self::Equal,
        }
    }
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
    /// Provisional IDs can already occur in parent shapes when an SCC seals.
    /// Redirects preserve those issued IDs while canonicalizing their meaning.
    Redirect(TypeId),
}

fn remap_type_shape(shape: &mut TypeShape, mut remap: impl FnMut(TypeId) -> TypeId) {
    match shape {
        TypeShape::Record(fields) => {
            for field in fields.values_mut() {
                field.final_type = remap(field.final_type);
            }
        }
        TypeShape::Variant(cases) => {
            for payload in cases.values_mut() {
                if let CasePayload::Record(type_id) = payload {
                    *type_id = remap(*type_id);
                }
            }
        }
        TypeShape::List { element, .. } | TypeShape::Option(element) => {
            *element = remap(*element);
        }
        TypeShape::Node | TypeShape::Text | TypeShape::Bool | TypeShape::Ref(_) => {}
    }
}

impl TypeAnalysis {
    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        let mut current = id;
        for _ in 0..=self.types.len() {
            match self.types.get(current.0 as usize)? {
                TypeEntry::ReservedNoValue => return None,
                TypeEntry::Shape(shape) => return Some(shape),
                TypeEntry::Redirect(target) => current = *target,
            }
        }
        panic!("type redirects must be acyclic")
    }

    fn canonical_type_id(&self, id: TypeId) -> TypeId {
        let mut current = id;
        for _ in 0..=self.types.len() {
            match self
                .types
                .get(current.0 as usize)
                .expect("canonicalized type id must be registered")
            {
                TypeEntry::ReservedNoValue | TypeEntry::Shape(_) => return current,
                TypeEntry::Redirect(target) => current = *target,
            }
        }
        panic!("type redirects must be acyclic")
    }

    pub fn expect_type_shape(&self, id: TypeId) -> &TypeShape {
        self.type_shape(id)
            .expect("admitted type id must reference a registered type")
    }

    /// Coinductive structural comparison over the type graph.
    ///
    /// Transparent aliases compare through their bodies, while declarations
    /// owning records or variants remain nominal. An open declaration yields
    /// [`TypeRelation::Pending`] instead of being mistaken for a distinct type.
    fn type_relation(&self, a: TypeId, b: TypeId) -> TypeRelation {
        self.type_relation_inner(a, b, &mut HashSet::new())
    }

    pub(crate) fn types_structurally_equal(&self, a: TypeId, b: TypeId) -> bool {
        match self.type_relation(a, b) {
            TypeRelation::Equal => true,
            TypeRelation::Distinct => false,
            TypeRelation::Pending => {
                panic!("frozen type graph cannot contain pending declarations")
            }
        }
    }

    fn type_relation_inner(
        &self,
        a: TypeId,
        b: TypeId,
        visiting: &mut HashSet<(TypeId, TypeId)>,
    ) -> TypeRelation {
        if a == b {
            return TypeRelation::Equal;
        }
        if !visiting.insert((a, b)) {
            return TypeRelation::Equal;
        }

        let result = match (self.comparison_operand(a), self.comparison_operand(b)) {
            (ComparisonOperand::Pending, _) | (_, ComparisonOperand::Pending) => {
                TypeRelation::Pending
            }
            (ComparisonOperand::Type(a), ComparisonOperand::Type(b)) if a == b => {
                TypeRelation::Equal
            }
            (ComparisonOperand::Type(a), ComparisonOperand::Type(b)) => {
                self.compare_resolved_types(a, b, visiting)
            }
        };

        visiting.remove(&(a, b));
        result
    }

    fn compare_resolved_types(
        &self,
        a: TypeId,
        b: TypeId,
        visiting: &mut HashSet<(TypeId, TypeId)>,
    ) -> TypeRelation {
        let (Some(shape_a), Some(shape_b)) = (self.type_shape(a), self.type_shape(b)) else {
            return TypeRelation::Distinct;
        };

        match (shape_a, shape_b) {
            (TypeShape::Node, TypeShape::Node)
            | (TypeShape::Text, TypeShape::Text)
            | (TypeShape::Bool, TypeShape::Bool) => TypeRelation::Equal,
            (TypeShape::Record(a_fields), TypeShape::Record(b_fields)) => {
                if a_fields.len() != b_fields.len() {
                    return TypeRelation::Distinct;
                }
                a_fields.iter().zip(b_fields).fold(
                    TypeRelation::Equal,
                    |relation, ((a_name, a_field), (b_name, b_field))| {
                        if a_name != b_name {
                            return TypeRelation::Distinct;
                        }
                        relation.and(self.type_relation_inner(
                            a_field.final_type,
                            b_field.final_type,
                            visiting,
                        ))
                    },
                )
            }
            (TypeShape::Variant(a_cases), TypeShape::Variant(b_cases)) => {
                if a_cases.len() != b_cases.len() {
                    return TypeRelation::Distinct;
                }
                a_cases.iter().zip(b_cases).fold(
                    TypeRelation::Equal,
                    |relation, ((a_name, a_payload), (b_name, b_payload))| {
                        if a_name != b_name {
                            return TypeRelation::Distinct;
                        }
                        let payload_relation = match (a_payload.type_id(), b_payload.type_id()) {
                            (None, None) => TypeRelation::Equal,
                            (Some(a), Some(b)) => self.type_relation_inner(a, b, visiting),
                            _ => TypeRelation::Distinct,
                        };
                        relation.and(payload_relation)
                    },
                )
            }
            (
                TypeShape::List {
                    element: a_element,
                    minimum: a_minimum,
                },
                TypeShape::List {
                    element: b_element,
                    minimum: b_minimum,
                },
            ) => {
                if a_minimum != b_minimum {
                    return TypeRelation::Distinct;
                }
                self.type_relation_inner(*a_element, *b_element, visiting)
            }
            (TypeShape::Option(a_inner), TypeShape::Option(b_inner)) => {
                self.type_relation_inner(*a_inner, *b_inner, visiting)
            }
            (TypeShape::Ref(a), TypeShape::Ref(b)) if a == b => TypeRelation::Equal,
            _ => TypeRelation::Distinct,
        }
    }

    fn comparison_operand(&self, mut type_id: TypeId) -> ComparisonOperand {
        let mut seen = HashSet::new();
        loop {
            type_id = self.canonical_type_id(type_id);
            let Some(TypeShape::Ref(declaration)) = self.type_shape(type_id) else {
                return ComparisonOperand::Type(type_id);
            };
            if !seen.insert(*declaration) {
                return ComparisonOperand::Type(type_id);
            }
            let entry = self
                .declarations
                .get(declaration.index())
                .expect("compared reference target must be registered");
            match entry.state {
                DeclarationState::Pending => return ComparisonOperand::Pending,
                DeclarationState::MatchOnly => type_id = TYPE_NODE,
                DeclarationState::Value(body) => {
                    if entry.owner != DeclarationOwner::Inference
                        && matches!(
                            self.type_shape(body),
                            Some(TypeShape::Record(_) | TypeShape::Variant(_))
                        )
                    {
                        return ComparisonOperand::Type(type_id);
                    }
                    type_id = body;
                }
            }
        }
    }

    pub fn declaration(&self, id: TypeDeclId) -> Option<TypeDeclaration> {
        let entry = self.declarations.get(id.index())?;
        Some(TypeDeclaration {
            id,
            name: entry.name?,
            body: entry.state.value()?,
        })
    }

    pub fn declaration_body(&self, id: TypeDeclId) -> Option<TypeId> {
        self.declarations
            .get(id.index())
            .and_then(|entry| entry.state.value())
    }

    pub fn declaration_name(&self, id: TypeDeclId) -> Symbol {
        self.declarations
            .get(id.index())
            .expect("type declaration id must be registered")
            .name
            .expect("named type declaration must have a name")
    }

    pub fn declaration_definition(&self, id: TypeDeclId) -> Option<DefId> {
        match self
            .declarations
            .get(id.index())
            .expect("type declaration id must be registered")
            .owner
        {
            DeclarationOwner::Definition(def_id) => Some(def_id),
            DeclarationOwner::CaptureType | DeclarationOwner::Inference => None,
        }
    }

    pub fn definition_declaration(&self, def_id: DefId) -> TypeDeclId {
        *self
            .definition_declarations
            .get(&def_id)
            .expect("every definition must own a type declaration slot")
    }

    fn reachable_option(&self, mut type_id: TypeId) -> Option<TypeId> {
        let mut seen = HashSet::new();
        loop {
            type_id = self.canonical_type_id(type_id);
            if !seen.insert(type_id) {
                return None;
            }
            match self.type_shape(type_id) {
                Some(TypeShape::Option(_)) => return Some(type_id),
                Some(TypeShape::Ref(declaration)) => {
                    let target = self.declaration_body(*declaration)?;
                    type_id = target;
                }
                _ => return None,
            }
        }
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

    pub fn def_output(&self, def_id: DefId) -> Option<DefinitionOutput> {
        let declaration = *self.definition_declarations.get(&def_id)?;
        self.declarations
            .get(declaration.index())
            .and_then(|entry| entry.state.output())
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
        self.definition_declarations
            .iter()
            .map(|(&def_id, &declaration)| {
                let output = self.declarations[declaration.index()]
                    .state
                    .output()
                    .expect("admitted definition output must be sealed");
                (def_id, output)
            })
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

        for (index, entry) in self.types.iter().enumerate() {
            let shape = match entry {
                TypeEntry::ReservedNoValue => continue,
                TypeEntry::Redirect(target) => {
                    self.assert_type_id_registered(*target, "redirect target type id out of range");
                    assert_eq!(
                        self.canonical_type_id(*target),
                        *target,
                        "sealed type redirects must point directly to canonical types",
                    );
                    continue;
                }
                TypeEntry::Shape(shape) => shape,
            };
            for child_id in shape.child_type_ids() {
                self.assert_type_id_registered(child_id, "child type id out of range");
            }

            if let TypeShape::Option(inner) = shape {
                let outer = TypeId(u32::try_from(index).expect("type count fits u32"));
                assert!(
                    self.reachable_option(*inner)
                        .is_none_or(|reachable| reachable == outer),
                    "Option must be idempotent",
                );
            }

            if let TypeShape::Ref(declaration) = shape {
                let target = self
                    .declarations
                    .get(declaration.index())
                    .expect("every Ref target must be a registered type declaration");
                assert_ne!(
                    target.owner,
                    DeclarationOwner::Inference,
                    "sealed inference references must redirect to their resolved types",
                );
                assert!(
                    target.name.is_some(),
                    "every sealed Ref target must have a public name",
                );
            }
        }

        for (_, output) in self.iter_def_output() {
            if let DefinitionOutput::Value(type_id) = output {
                self.assert_type_id_registered(type_id, "definition result type id out of range");
            }
        }

        for (&def_id, &declaration) in &self.definition_declarations {
            let entry = self
                .declarations
                .get(declaration.index())
                .expect("definition declaration id must be registered");
            assert_eq!(entry.owner, DeclarationOwner::Definition(def_id));
            assert!(
                entry.state.output().is_some(),
                "every definition declaration must be sealed",
            );
        }
        for declaration in &self.declarations {
            assert_ne!(
                declaration.state,
                DeclarationState::Pending,
                "every type declaration must be sealed",
            );
            match declaration.owner {
                DeclarationOwner::Definition(_) | DeclarationOwner::CaptureType => {
                    assert!(
                        declaration.name.is_some(),
                        "public type declarations must retain their names",
                    );
                }
                DeclarationOwner::Inference => {
                    assert!(
                        declaration.name.is_none(),
                        "inference declarations are anonymous",
                    );
                }
            }
        }

        assert_eq!(
            self.definition_declarations.len(),
            self.def_root_extent.len(),
            "definition output and root-extent tables must cover the same definitions",
        );
        assert_eq!(
            self.definition_declarations.len(),
            self.def_requires_anchor_context.len(),
            "definition output and anchor-context tables must cover the same definitions",
        );
        for def_id in self.definition_declarations.keys() {
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
                self.definition_declarations.contains_key(def_id),
                "every definition root extent must have an inferred output",
            );
        }
        for def_id in self.def_requires_anchor_context.keys() {
            assert!(
                self.definition_declarations.contains_key(def_id),
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
/// frozen result does not: interning indexes, the open SCC, and its deferred
/// constraints. [`finish`](Self::finish) drops the scratch and hands back the
/// frozen [`TypeAnalysis`].
pub struct TypeAnalysisBuilder {
    pub(super) analysis: TypeAnalysis,

    /// Reverse index for `intern_type` deduplication of leaf and wrapper shapes.
    /// Records and variant types are deliberately NOT deduplicated: they are nominal —
    /// two definitions with identical capture profiles are two distinct types,
    /// each carrying its own name. Scratch: the frozen result looks types up by
    /// `TypeId`, never by shape. SCC sealing redirects provisional IDs and
    /// rebuilds this index from the canonical wrapper graph.
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

    open_scc: Option<HashSet<DefId>>,
    deferred_unifications: Vec<DeferredUnification>,
}

struct DeferredUnification {
    declaration: TypeDeclId,
    reference: TypeId,
    left: TypeId,
    right: TypeId,
    error: UnifyError,
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
            open_scc: None,
            deferred_unifications: Vec::new(),
        };

        // Preserve bytecode primitive numbering without treating no-value flow as a type.
        builder.analysis.types.push(TypeEntry::ReservedNoValue);
        assert_eq!(
            builder.analysis.types.len(),
            TYPE_NODE.0 as usize,
            "reserved type layout changed before interning `Node`; the bytecode primitive IDs \
             require `Node` at TYPE_NODE"
        );

        let node_id = builder.intern_type(TypeShape::Node);
        assert_eq!(
            node_id, TYPE_NODE,
            "reserved type layout assigned `Node` the wrong bytecode primitive ID"
        );

        let text_id = builder.intern_type(TypeShape::Text);
        assert_eq!(
            text_id, TYPE_TEXT,
            "reserved type layout assigned `text` the wrong bytecode primitive ID"
        );

        let bool_id = builder.intern_type(TypeShape::Bool);
        assert_eq!(
            bool_id, TYPE_BOOL,
            "reserved type layout assigned `bool` the wrong bytecode primitive ID"
        );

        builder
    }

    /// Freeze the accumulated state, dropping the inference-only scratch. Admits
    /// the result only after asserting it is internally consistent.
    pub fn finish(self) -> TypeAnalysis {
        assert!(
            self.open_scc.is_none(),
            "every SCC must be sealed before finish"
        );
        assert!(
            self.deferred_unifications.is_empty(),
            "every deferred type constraint must be resolved before finish",
        );
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

    /// Open the boundary within which inference may observe pending declarations.
    pub(super) fn begin_scc(&mut self, definitions: &[DefId]) {
        assert!(
            self.open_scc.is_none(),
            "SCCs are sealed before the next opens"
        );
        assert!(
            self.deferred_unifications.is_empty(),
            "deferred constraints belong to exactly one SCC",
        );
        let definitions = definitions.iter().copied().collect::<HashSet<_>>();
        assert!(
            definitions.iter().all(|def_id| {
                let declaration = self.analysis.definition_declaration(*def_id);
                self.analysis.declarations[declaration.index()].state == DeclarationState::Pending
            }),
            "an SCC opens only while all of its definition outputs are pending",
        );
        self.open_scc = Some(definitions);
    }

    /// Resolve every order-dependent decision before later passes snapshot the graph.
    pub(super) fn seal_scc(&mut self) -> Vec<UnifyError> {
        let definitions = self
            .open_scc
            .take()
            .expect("an SCC must be open before sealing");
        assert!(
            definitions.iter().all(|def_id| {
                let declaration = self.analysis.definition_declaration(*def_id);
                self.analysis.declarations[declaration.index()]
                    .state
                    .output()
                    .is_some()
            }),
            "every definition output in an SCC must be registered before sealing",
        );

        let errors = self.resolve_deferred_unifications();
        self.normalize_sealed_type_graph();
        errors
    }

    fn resolve_deferred_unifications(&mut self) -> Vec<UnifyError> {
        let constraints = std::mem::take(&mut self.deferred_unifications);
        let mut errors = Vec::new();
        let mut failed = HashSet::new();
        for constraint in constraints {
            let left_failed = self.type_depends_on_any(constraint.left, &failed);
            let right_failed = self.type_depends_on_any(constraint.right, &failed);
            let resolved = if left_failed || right_failed {
                failed.insert(constraint.reference);
                self.analysis.canonical_type_id(if left_failed {
                    constraint.right
                } else {
                    constraint.left
                })
            } else {
                match self.unify_types(constraint.left, constraint.right) {
                    TypeUnification::Unified(type_id) => type_id,
                    TypeUnification::Distinct => {
                        failed.insert(constraint.reference);
                        errors.push(constraint.error);
                        self.analysis.canonical_type_id(constraint.left)
                    }
                    TypeUnification::Pending => {
                        panic!("sealing an SCC must resolve every deferred type constraint")
                    }
                }
            };
            self.analysis.declarations[constraint.declaration.index()].state =
                DeclarationState::Value(resolved);
            self.redirect_type(constraint.reference, resolved);
        }
        errors
    }

    fn type_depends_on_any(&self, type_id: TypeId, targets: &HashSet<TypeId>) -> bool {
        let mut pending = vec![type_id];
        let mut seen = HashSet::new();
        while let Some(type_id) = pending.pop() {
            if targets.contains(&type_id) {
                return true;
            }
            if !seen.insert(type_id) {
                continue;
            }
            match self
                .analysis
                .types
                .get(type_id.0 as usize)
                .expect("deferred constraint type must be registered")
            {
                TypeEntry::ReservedNoValue => {}
                TypeEntry::Redirect(target) => pending.push(*target),
                TypeEntry::Shape(shape) => pending.extend(shape.child_type_ids()),
            }
        }
        false
    }

    pub(super) fn unify_types(&mut self, a: TypeId, b: TypeId) -> TypeUnification {
        match self.analysis.type_relation(a, b) {
            TypeRelation::Equal => {
                return TypeUnification::Unified(self.analysis.canonical_type_id(a));
            }
            TypeRelation::Pending => return TypeUnification::Pending,
            TypeRelation::Distinct => {}
        }

        let (ComparisonOperand::Type(a), ComparisonOperand::Type(b)) = (
            self.analysis.comparison_operand(a),
            self.analysis.comparison_operand(b),
        ) else {
            return TypeUnification::Pending;
        };
        let a_shape = self
            .analysis
            .type_shape(a)
            .cloned()
            .expect("unified field type must be registered");
        let b_shape = self
            .analysis
            .type_shape(b)
            .cloned()
            .expect("unified field type must be registered");

        match (a_shape, b_shape) {
            (TypeShape::Option(a_inner), TypeShape::Option(b_inner)) => {
                self.unify_option_inners(a_inner, b_inner)
            }
            (TypeShape::Option(inner), _) => self.unify_option_inners(inner, b),
            (_, TypeShape::Option(inner)) => self.unify_option_inners(a, inner),
            (
                TypeShape::List {
                    element: a_element,
                    minimum: a_minimum,
                },
                TypeShape::List {
                    element: b_element,
                    minimum: b_minimum,
                },
            ) if a_minimum != b_minimum => {
                match self.analysis.type_relation(a_element, b_element) {
                    TypeRelation::Equal => {
                        TypeUnification::Unified(if a_minimum == ListMinimum::One { b } else { a })
                    }
                    TypeRelation::Pending => TypeUnification::Pending,
                    TypeRelation::Distinct => TypeUnification::Distinct,
                }
            }
            _ => TypeUnification::Distinct,
        }
    }

    fn unify_option_inners(&mut self, a: TypeId, b: TypeId) -> TypeUnification {
        match self.unify_types(a, b) {
            TypeUnification::Unified(inner) => TypeUnification::Unified(self.intern_option(inner)),
            TypeUnification::Distinct => TypeUnification::Distinct,
            TypeUnification::Pending => TypeUnification::Pending,
        }
    }

    pub(in crate::compiler::analyze::types) fn defer_unification(
        &mut self,
        left: TypeId,
        right: TypeId,
        error: UnifyError,
    ) -> TypeId {
        assert!(
            self.open_scc.is_some(),
            "type constraints are deferred only while an SCC is open",
        );
        let declaration = TypeDeclId::from_raw(
            u32::try_from(self.analysis.declarations.len())
                .expect("type declaration count fits u32"),
        );
        self.analysis.declarations.push(TypeDeclarationEntry {
            name: None,
            state: DeclarationState::Pending,
            owner: DeclarationOwner::Inference,
        });
        let reference = self.intern_type(TypeShape::Ref(declaration));
        self.deferred_unifications.push(DeferredUnification {
            declaration,
            reference,
            left,
            right,
            error,
        });
        reference
    }

    fn redirect_type(&mut self, from: TypeId, to: TypeId) {
        let to = self.analysis.canonical_type_id(to);
        assert_ne!(from, to, "a type cannot redirect to itself");
        let entry = self
            .analysis
            .types
            .get_mut(from.0 as usize)
            .expect("redirected type id must be registered");
        *entry = TypeEntry::Redirect(to);
    }

    /// Intern a type shape. Leaf and wrapper shapes deduplicate structurally;
    /// records and variant types always mint a fresh id (they are nominal — see the
    /// `intern_index` field docs).
    pub fn intern_type(&mut self, mut shape: TypeShape) -> TypeId {
        self.canonicalize_shape(&mut shape);
        if let TypeShape::Option(inner) = &shape
            && self.analysis.reachable_option(*inner).is_some()
        {
            return self.analysis.canonical_type_id(*inner);
        }

        if matches!(shape, TypeShape::Record(_) | TypeShape::Variant(_)) {
            let id = TypeId(self.analysis.types.len() as u32);
            self.analysis.types.push(TypeEntry::Shape(shape));
            return id;
        }

        if let Some(&id) = self.intern_index.get(&shape) {
            return self.analysis.canonical_type_id(id);
        }

        let id = TypeId(self.analysis.types.len() as u32);
        self.analysis.types.push(TypeEntry::Shape(shape.clone()));
        self.intern_index.insert(shape, id);
        id
    }

    fn canonicalize_shape(&self, shape: &mut TypeShape) {
        remap_type_shape(shape, |type_id| self.analysis.canonical_type_id(type_id));
    }

    pub(super) fn type_shapes_snapshot(&self) -> Vec<Option<TypeShape>> {
        self.analysis
            .types
            .iter()
            .enumerate()
            .map(|(index, entry)| match entry {
                TypeEntry::ReservedNoValue => None,
                TypeEntry::Shape(_) | TypeEntry::Redirect(_) => self
                    .analysis
                    .type_shape(TypeId(u32::try_from(index).expect("type count fits u32")))
                    .cloned(),
            })
            .collect()
    }

    pub fn intern_option(&mut self, inner: TypeId) -> TypeId {
        self.intern_type(TypeShape::Option(inner))
    }

    /// Restore wrapper idempotence and interning uniqueness after redirects
    /// reveal declaration bodies that were opaque while the SCC was open.
    fn normalize_sealed_type_graph(&mut self) {
        for _ in 0..=self.analysis.types.len() {
            let options_changed = self.redirect_redundant_options();
            self.remap_registered_type_ids();
            let duplicates_changed = self.rebuild_intern_index();
            if !options_changed && !duplicates_changed {
                self.remap_registered_type_ids();
                return;
            }
        }
        panic!("sealed type canonicalization must converge")
    }

    /// Type IDs follow creation order, so processing options forwards preserves
    /// one wrapper in a recursive wrapper cycle instead of redirecting the
    /// entire cycle into references.
    fn redirect_redundant_options(&mut self) -> bool {
        let mut changed = false;
        for index in 0..self.analysis.types.len() {
            let outer = TypeId(u32::try_from(index).expect("type count fits u32"));
            let Some(TypeEntry::Shape(TypeShape::Option(inner))) =
                self.analysis.types.get(index).cloned()
            else {
                continue;
            };
            let Some(reachable) = self.analysis.reachable_option(inner) else {
                continue;
            };
            if reachable == outer {
                continue;
            }

            self.redirect_type(outer, inner);
            changed = true;
        }
        changed
    }

    fn remap_registered_type_ids(&mut self) {
        let canonical = (0..self.analysis.types.len())
            .map(|index| {
                self.analysis
                    .canonical_type_id(TypeId(u32::try_from(index).expect("type count fits u32")))
            })
            .collect::<Vec<_>>();
        let remap = |type_id: TypeId| {
            *canonical
                .get(type_id.0 as usize)
                .expect("stored type id must be registered")
        };

        for entry in &mut self.analysis.types {
            match entry {
                TypeEntry::Shape(shape) => remap_type_shape(shape, remap),
                TypeEntry::Redirect(target) => *target = remap(*target),
                TypeEntry::ReservedNoValue => {}
            }
        }
        for declaration in &mut self.analysis.declarations {
            if let DeclarationState::Value(type_id) = &mut declaration.state {
                *type_id = remap(*type_id);
            }
        }
        for shape in self.analysis.pattern_result.values_mut() {
            match &mut shape.flow {
                PatternFlow::Value(type_id) | PatternFlow::Fields(type_id) => {
                    *type_id = remap(*type_id);
                }
                PatternFlow::NoValue => {}
            }
            let Some(field_flow) = &mut shape.field_flow else {
                continue;
            };
            field_flow.type_id = remap(field_flow.type_id);
            for field in field_flow.fields.values_mut() {
                field.info.final_type = remap(field.info.final_type);
                for source in &mut field.sources {
                    match source {
                        FieldSource::Capture { info, .. } | FieldSource::Forwarded { info, .. } => {
                            info.final_type = remap(info.final_type);
                        }
                    }
                }
            }
        }
        for occurrence in &mut self.custom_capture_types {
            occurrence.type_id = remap(occurrence.type_id);
        }

        let mut provenance = std::mem::take(&mut self.type_provenance)
            .into_iter()
            .collect::<Vec<_>>();
        provenance.sort_by_key(|&(type_id, _)| type_id);
        for (type_id, span) in provenance {
            self.type_provenance.entry(remap(type_id)).or_insert(span);
        }

        let mut capture_declarations = std::mem::take(&mut self.capture_type_declarations)
            .into_iter()
            .collect::<Vec<_>>();
        capture_declarations.sort_by_key(|&(_, declaration)| declaration);
        for ((name, body), declaration) in capture_declarations {
            self.capture_type_declarations
                .entry((name, remap(body)))
                .or_insert(declaration);
        }
        self.invalid_types = std::mem::take(&mut self.invalid_types)
            .into_iter()
            .map(remap)
            .collect();
        for (type_id, name) in std::mem::take(&mut self.analysis.named_types) {
            self.analysis
                .named_types
                .entry(remap(type_id))
                .or_insert(name);
        }
    }

    fn rebuild_intern_index(&mut self) -> bool {
        let mut index = HashMap::new();
        let mut redirects = Vec::new();
        for (raw, entry) in self.analysis.types.iter().enumerate() {
            let TypeEntry::Shape(shape) = entry else {
                continue;
            };
            if matches!(shape, TypeShape::Record(_) | TypeShape::Variant(_)) {
                continue;
            }
            let type_id = TypeId(u32::try_from(raw).expect("type count fits u32"));
            if let Some(&existing) = index.get(shape) {
                redirects.push((type_id, existing));
            } else {
                index.insert(shape.clone(), type_id);
            }
        }
        for (duplicate, canonical) in &redirects {
            self.redirect_type(*duplicate, *canonical);
        }
        self.intern_index = index;
        !redirects.is_empty()
    }

    pub fn intern_record(&mut self, fields: BTreeMap<Symbol, RecordField>) -> TypeId {
        self.intern_type(TypeShape::Record(fields))
    }

    pub(super) fn replace_record_fields(
        &mut self,
        type_id: TypeId,
        mut fields: BTreeMap<Symbol, RecordField>,
    ) {
        let type_id = self.analysis.canonical_type_id(type_id);
        for field in fields.values_mut() {
            field.final_type = self.analysis.canonical_type_id(field.final_type);
        }
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
                name: Some(name),
                state: DeclarationState::Pending,
                owner: DeclarationOwner::Definition(def_id),
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
            name: Some(name),
            state: DeclarationState::Value(body),
            owner: DeclarationOwner::CaptureType,
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
        assert!(
            self.open_scc
                .as_ref()
                .is_some_and(|definitions| definitions.contains(&def_id)),
            "a definition output is registered only while its SCC is open",
        );
        let declaration = self.analysis.definition_declaration(def_id);
        let entry = &mut self.analysis.declarations[declaration.index()];
        assert_eq!(
            entry.state,
            DeclarationState::Pending,
            "a definition output is registered exactly once",
        );
        entry.state = DeclarationState::from_output(output);
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
