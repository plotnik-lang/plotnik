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

use crate::compiler::core::ast::Pattern;
use crate::compiler::core::type_shape::{
    Arity, FieldInfo, PatternResult, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::core::{DefId, Symbol};

/// Frozen registry of inferred types and per-definition / per-pattern results.
///
/// Constructed only via [`TypeAnalysisBuilder`]; the private fields and
/// `#[non_exhaustive]` keep that the single entry point.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TypeAnalysis {
    types: Vec<TypeShape>,

    /// Each definition's output type, keyed by `DefId`. `BTreeMap` so iteration
    /// is in `DefId` order — the SCC/emission order entrypoints rely on. A
    /// value-less body (`.`, `-field`) has no entry, so a lookup can legitimately
    /// miss; that absence is meaningful, not an invariant violation.
    def_output: BTreeMap<DefId, TypeId>,

    pattern_result: HashMap<Pattern, PatternResult>,

    /// Explicit type aliases from annotations like `{...} @x :: TypeName`.
    /// Maps a struct/enum `TypeId` to the name it should have in generated code.
    type_aliases: HashMap<TypeId, Symbol>,
}

impl TypeAnalysis {
    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.types.get(id.0 as usize)
    }

    pub fn struct_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, FieldInfo>> {
        match self.type_shape(id)? {
            TypeShape::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Fields of the struct a `Fields` flow points to.
    ///
    /// Every `OutputFlow::Fields` is constructed by interning a `Struct` (see the
    /// `intern_struct`/`intern_single_field` calls at every `Fields` construction
    /// site), so a non-`Struct` id here is a broken type-system invariant, not a
    /// runtime condition the query can trigger. We surface it loudly instead of
    /// fabricating an empty struct that would silently mistype the output.
    pub fn expect_struct_fields(&self, id: TypeId) -> &BTreeMap<Symbol, FieldInfo> {
        self.struct_fields(id)
            .expect("Fields flow must point to a Struct type")
    }

    /// Whether a type is a meaningful structured output (enum/struct/ref, or an
    /// array/optional thereof). Plain `Node` is not — it is the matched node,
    /// captured directly.
    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        match self.type_shape(type_id) {
            Some(TypeShape::Enum(_) | TypeShape::Struct(_) | TypeShape::Ref(_)) => true,
            Some(TypeShape::Array { element, .. }) => {
                *element != TYPE_NODE && self.is_structured_output(*element)
            }
            Some(TypeShape::Optional(inner)) => {
                *inner != TYPE_NODE && self.is_structured_output(*inner)
            }
            _ => false,
        }
    }

    pub fn pattern_result(&self, pattern: &Pattern) -> Option<&PatternResult> {
        self.pattern_result.get(pattern)
    }

    pub fn arity(&self, pattern: &Pattern) -> Option<Arity> {
        self.pattern_result.get(pattern).map(|info| info.arity)
    }

    pub fn def_output(&self, def_id: DefId) -> Option<TypeId> {
        self.def_output.get(&def_id).copied()
    }

    /// Follow a `Ref` chain to the underlying materialized type; non-ref types
    /// resolve to themselves. The accessor type-table emission uses to map a
    /// query type to the concrete shape it stands for.
    pub fn resolve_underlying_type_id(&self, type_id: TypeId) -> TypeId {
        let Some(TypeShape::Ref(def_id)) = self.type_shape(type_id) else {
            return type_id;
        };
        let target = self
            .def_output(*def_id)
            .expect("ref target def type must exist");
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

    /// Each definition's full inferred `PatternResult`, keyed by `DefId`. Lets a
    /// non-recursive `Ref` return its target's result (arity + flow, fields
    /// intact for bubbling) without re-descending into the referenced body.
    /// Scratch: only the inference walk consults it.
    def_memo: HashMap<DefId, PatternResult>,
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

    /// Freeze the accumulated state, dropping the inference-only scratch.
    pub fn finish(self) -> TypeAnalysis {
        self.analysis
    }

    /// Read-only view of the in-progress artifact, for the shared accessors
    /// ([`TypeAnalysis::capture_mechanism`], [`TypeAnalysis::ref_returns_structured`])
    /// that also run against the frozen result during emission.
    pub fn analysis(&self) -> &TypeAnalysis {
        &self.analysis
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

    pub fn record_pattern_result(&mut self, pattern: Pattern, info: PatternResult) {
        self.analysis.pattern_result.insert(pattern, info);
    }

    pub fn set_def_output(&mut self, def_id: DefId, type_id: TypeId) {
        self.analysis.def_output.insert(def_id, type_id);
    }

    /// Record a definition's full inferred result, so non-recursive references
    /// can resolve to it instead of re-descending into the body.
    pub fn set_def_memo(&mut self, def_id: DefId, info: PatternResult) {
        self.def_memo.insert(def_id, info);
    }

    pub fn def_memo(&self, def_id: DefId) -> Option<&PatternResult> {
        self.def_memo.get(&def_id)
    }

    /// Associate an explicit alias with a type (from `@x :: TypeName` on struct captures).
    pub fn set_type_alias(&mut self, type_id: TypeId, name: Symbol) {
        self.analysis.type_aliases.insert(type_id, name);
    }

    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.analysis.type_shape(id)
    }

    pub fn expect_struct_fields(&self, id: TypeId) -> &BTreeMap<Symbol, FieldInfo> {
        self.analysis.expect_struct_fields(id)
    }

    pub fn is_structured_output(&self, type_id: TypeId) -> bool {
        self.analysis.is_structured_output(type_id)
    }

    pub fn pattern_result(&self, pattern: &Pattern) -> Option<&PatternResult> {
        self.analysis.pattern_result(pattern)
    }

    pub fn def_output(&self, def_id: DefId) -> Option<TypeId> {
        self.analysis.def_output(def_id)
    }
}
