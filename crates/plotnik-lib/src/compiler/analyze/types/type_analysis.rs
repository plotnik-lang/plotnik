//! `TypeAnalysis`: the frozen result of type inference.
//!
//! Holds the interned type registry, each definition's output type, the
//! per-pattern inference results, and explicit type aliases. It is built
//! incrementally by [`TypeAnalysisBuilder`] and frozen with
//! [`TypeAnalysisBuilder::finish`]; past that boundary it is immutable and its
//! accessors are trusted (a structural miss is a compiler bug, not a query
//! condition).
//!
//! Definition *identity* (names, `DefId`s, recursion) is not stored here — it is
//! owned by `DependencyAnalysis` and read from there. This artifact only maps the
//! `DefId`s that analysis already assigned to the types it inferred for them.

use std::collections::{BTreeMap, HashMap};

use crate::compiler::analyze::types::type_shape::{
    Arity, FieldInfo, PatternFlow, PatternShape, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::Pattern;
use crate::core::Symbol;

/// Frozen registry of inferred types and per-definition / per-pattern results.
///
/// Constructed only via [`TypeAnalysisBuilder`]; the private fields and
/// `#[non_exhaustive]` keep that the single entry point.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TypeAnalysis {
    types: Vec<TypeShape>,

    /// Each definition's output type, keyed by `DefId`. `BTreeMap` so iteration
    /// is in `DefId` order — the SCC/emission order entrypoints rely on. Total
    /// over every scheduled definition: a value-less body (`.`, `-field`) maps to
    /// `TYPE_VOID`, so a lookup for any real `DefId` hits. `finish` admits the map
    /// only after checking its type ids and every `Ref` target are consistent.
    def_output: BTreeMap<DefId, TypeId>,

    pattern_result: HashMap<Pattern, PatternShape>,

    /// Explicit type aliases from annotations like `{...} @x :: TypeName`.
    /// Maps a struct/enum `TypeId` to the name it should have in generated code.
    type_aliases: HashMap<TypeId, Symbol>,
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

    /// Whether a type is a meaningful structured output (enum/struct/ref, or an
    /// array/optional thereof). Plain `Node` is not — it is the matched node,
    /// captured directly.
    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        match self.type_shape(type_id) {
            Some(TypeShape::Enum(_) | TypeShape::Struct(_) | TypeShape::Ref(_)) => true,
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

    /// Follow a `Ref` chain to the underlying materialized type; non-ref types
    /// resolve to themselves. The accessor type-table emission uses to map a
    /// query type to the concrete shape it stands for.
    pub fn resolve_underlying_type_id(&self, type_id: TypeId) -> TypeId {
        let Some(TypeShape::Ref(def_id)) = self.type_shape(type_id) else {
            return type_id;
        };
        let target = self.expect_def_output(*def_id);
        self.resolve_underlying_type_id(target)
    }

    /// Iterate over all definition output types as `(DefId, TypeId)` in `DefId`
    /// order, which corresponds to SCC processing order (leaves first).
    pub fn iter_def_output(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        self.def_output.iter().map(|(&id, &type_id)| (id, type_id))
    }

    pub fn iter_type_aliases(&self) -> impl Iterator<Item = (TypeId, Symbol)> + '_ {
        self.type_aliases.iter().map(|(&id, &sym)| (id, sym))
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

        for info in self.pattern_result.values() {
            self.assert_flow_well_formed(&info.flow);
        }

        for &type_id in self.type_aliases.keys() {
            self.assert_type_id_registered(type_id, "type alias type id out of range");
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
    analysis: TypeAnalysis,

    /// Reverse index for `intern_type` deduplication. Scratch: the frozen result
    /// looks types up by `TypeId`, never by shape.
    intern_index: HashMap<TypeShape, TypeId>,

    /// Each definition's full inferred `PatternShape`, keyed by `DefId`. Lets a
    /// non-recursive `Ref` return its target's result (arity + flow, fields
    /// intact for bubbling) without re-descending into the referenced body.
    /// Scratch: only the inference walk consults it.
    def_memo: HashMap<DefId, PatternShape>,
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

    pub(crate) fn is_structured_output(&self, type_id: TypeId) -> bool {
        self.analysis.is_structured_output(type_id)
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
                pattern_result: HashMap::new(),
                type_aliases: HashMap::new(),
            },
            intern_index: HashMap::new(),
            def_memo: HashMap::new(),
        };

        // Pre-register builtin types at their expected IDs.
        let void_id = builder.intern_type(TypeShape::Void);
        debug_assert_eq!(void_id, TYPE_VOID);

        let node_id = builder.intern_type(TypeShape::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        builder
    }

    /// Freeze the accumulated state, dropping the inference-only scratch. Admits
    /// the result only after asserting it is internally consistent.
    pub fn finish(self) -> TypeAnalysis {
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

    /// Intern a type shape, deduplicating by structural equality.
    pub fn intern_type(&mut self, shape: TypeShape) -> TypeId {
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

    pub fn record_pattern_result(&mut self, pattern: Pattern, shape: PatternShape) {
        self.analysis.pattern_result.insert(pattern, shape);
    }

    pub fn record_def_output(&mut self, def_id: DefId, type_id: TypeId) {
        self.analysis.def_output.insert(def_id, type_id);
    }

    /// Record a definition's full inferred result, so non-recursive references
    /// can resolve to it instead of re-descending into the body.
    pub fn record_def_memo(&mut self, def_id: DefId, shape: PatternShape) {
        self.def_memo.insert(def_id, shape);
    }

    pub fn def_memo(&self, def_id: DefId) -> Option<&PatternShape> {
        self.def_memo.get(&def_id)
    }

    /// Associate an explicit alias with a type (from `@x :: TypeName` on struct captures).
    pub fn define_type_alias(&mut self, type_id: TypeId, name: Symbol) {
        self.analysis.type_aliases.insert(type_id, name);
    }
}
