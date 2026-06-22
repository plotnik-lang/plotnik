//! TypeContext: manages interned types, symbols, and term info cache.
//!
//! Types are interned to enable cheap equality checks and cycle handling.
//! Symbols are stored but resolved via external Interner reference.
//! TermInfo is cached per-expression to avoid recomputation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::Pattern;

use crate::type_shape::{Arity, FieldInfo, TYPE_NODE, TYPE_VOID, TermInfo, TypeId, TypeShape};
use crate::{DefId, Interner, Symbol};

/// Central registry for types, symbols, and expression metadata.
#[derive(Clone, Debug)]
pub struct TypeContext {
    types: Vec<TypeShape>,
    type_map: HashMap<TypeShape, TypeId>,

    def_names: Vec<Symbol>,
    def_ids: HashMap<Symbol, DefId>,
    /// Definition-level type info (for TypeScript emission), keyed by DefId
    def_types: HashMap<DefId, TypeId>,
    /// Full inferred TermInfo per definition, keyed by DefId. Lets a non-recursive
    /// `Ref` return its target's result (Arity + TypeFlow, fields intact for
    /// bubbling) without re-descending into the referenced body. Analysis-only:
    /// `def_types` stays the sole ordering source for emission.
    def_results: HashMap<DefId, TermInfo>,
    /// Definitions that are part of a recursive SCC
    recursive_defs: HashSet<DefId>,

    term_info: HashMap<Pattern, TermInfo>,

    /// Explicit type names from annotations like `{...} @x :: TypeName`.
    /// Maps a struct/enum TypeId to the name it should have in generated code.
    type_names: HashMap<TypeId, Symbol>,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    pub fn new() -> Self {
        let mut ctx = Self {
            types: Vec::new(),
            type_map: HashMap::new(),
            def_names: Vec::new(),
            def_ids: HashMap::new(),
            def_types: HashMap::new(),
            def_results: HashMap::new(),
            recursive_defs: HashSet::new(),
            term_info: HashMap::new(),
            type_names: HashMap::new(),
        };

        // Pre-register builtin types at their expected IDs
        let void_id = ctx.intern_type(TypeShape::Void);
        debug_assert_eq!(void_id, TYPE_VOID);

        let node_id = ctx.intern_type(TypeShape::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        ctx
    }

    /// Avoids re-registering definitions that DependencyAnalysis already assigned DefIds to.
    pub fn seed_defs(&mut self, def_names: &[Symbol], def_ids_by_sym: &HashMap<Symbol, DefId>) {
        self.def_names = def_names.to_vec();
        self.def_ids = def_ids_by_sym.clone();
    }

    /// Intern a type shape, deduplicating by structural equality.
    pub fn intern_type(&mut self, shape: TypeShape) -> TypeId {
        if let Some(&id) = self.type_map.get(&shape) {
            return id;
        }

        let id = TypeId(self.types.len() as u32);
        self.types.push(shape.clone());
        self.type_map.insert(shape, id);
        id
    }

    pub fn type_shape(&self, id: TypeId) -> Option<&TypeShape> {
        self.types.get(id.0 as usize)
    }

    pub fn intern_type_seen(&mut self, shape: TypeShape) -> (TypeId, &TypeShape) {
        let id = self.intern_type(shape);
        (id, &self.types[id.0 as usize])
    }

    pub fn intern_struct(&mut self, fields: BTreeMap<Symbol, FieldInfo>) -> TypeId {
        self.intern_type(TypeShape::Struct(fields))
    }

    pub fn intern_single_field(&mut self, name: Symbol, info: FieldInfo) -> TypeId {
        self.intern_type(TypeShape::Struct(BTreeMap::from([(name, info)])))
    }

    pub fn struct_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, FieldInfo>> {
        match self.type_shape(id)? {
            TypeShape::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Fields of the struct a `Fields` flow points to.
    ///
    /// Every `TypeFlow::Fields` is constructed by interning a `Struct` (see the
    /// `intern_struct`/`intern_single_field` calls at every `Fields` construction
    /// site), so a non-`Struct` id here is a broken type-system invariant, not a
    /// runtime condition the query can trigger. We surface it loudly instead of
    /// fabricating an empty struct that would silently mistype the output.
    pub fn expect_struct_fields(&self, id: TypeId) -> &BTreeMap<Symbol, FieldInfo> {
        self.struct_fields(id)
            .expect("Fields flow must point to a Struct type")
    }

    pub fn cache_term_info(&mut self, pattern: Pattern, info: TermInfo) {
        self.term_info.insert(pattern, info);
    }

    pub fn term_info(&self, pattern: &Pattern) -> Option<&TermInfo> {
        self.term_info.get(pattern)
    }

    pub fn register_def(&mut self, interner: &mut Interner, name: &str) -> DefId {
        let sym = interner.intern(name);
        self.register_def_sym(sym)
    }

    pub fn register_def_sym(&mut self, sym: Symbol) -> DefId {
        if let Some(&def_id) = self.def_ids.get(&sym) {
            return def_id;
        }

        let def_id = DefId::from_raw(self.def_names.len() as u32);
        self.def_names.push(sym);
        self.def_ids.insert(sym, def_id);
        def_id
    }

    pub fn def_id_for_sym(&self, sym: Symbol) -> Option<DefId> {
        self.def_ids.get(&sym).copied()
    }

    /// Get DefId for a definition name. `def_ids` is keyed by `Symbol`, so the
    /// name is resolved through the same interner that populated it.
    pub fn def_id_for_name(&self, interner: &Interner, name: &str) -> Option<DefId> {
        let sym = interner.get(name)?;
        self.def_id_for_sym(sym)
    }

    pub fn def_name_sym(&self, def_id: DefId) -> Symbol {
        self.def_names[def_id.index()]
    }

    pub fn def_name<'a>(&self, interner: &'a Interner, def_id: DefId) -> &'a str {
        interner.resolve(self.def_names[def_id.index()])
    }

    pub fn mark_recursive(&mut self, def_id: DefId) {
        self.recursive_defs.insert(def_id);
    }

    pub fn is_recursive(&self, def_id: DefId) -> bool {
        self.recursive_defs.contains(&def_id)
    }

    pub fn set_def_type(&mut self, def_id: DefId, type_id: TypeId) {
        self.def_types.insert(def_id, type_id);
    }

    /// Registers the def if not already known.
    pub fn set_def_type_by_name(&mut self, interner: &mut Interner, name: &str, type_id: TypeId) {
        let def_id = self.register_def(interner, name);
        self.set_def_type(def_id, type_id);
    }

    pub fn def_type(&self, def_id: DefId) -> Option<TypeId> {
        self.def_types.get(&def_id).copied()
    }

    /// Record a definition's full inferred result, so non-recursive references
    /// can resolve to it instead of re-descending into the body.
    pub fn set_def_result(&mut self, def_id: DefId, info: TermInfo) {
        self.def_results.insert(def_id, info);
    }

    pub fn def_result(&self, def_id: DefId) -> Option<&TermInfo> {
        self.def_results.get(&def_id)
    }

    pub fn def_type_for_name(&self, interner: &Interner, name: &str) -> Option<TypeId> {
        let id = self.def_id_for_name(interner, name)?;
        self.def_type(id)
    }

    pub fn arity(&self, pattern: &Pattern) -> Option<Arity> {
        self.term_info.get(pattern).map(|info| info.arity)
    }

    /// Iterate over all definition types as (DefId, TypeId) in DefId order.
    /// DefId order corresponds to SCC processing order (leaves first).
    pub fn iter_def_types(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        (0..self.def_names.len()).filter_map(|i| {
            let def_id = DefId::from_raw(i as u32);
            self.def_types
                .get(&def_id)
                .map(|&type_id| (def_id, type_id))
        })
    }

    /// Number of registered definitions.
    pub fn def_count(&self) -> usize {
        self.def_names.len()
    }

    /// Associate an explicit name with a type (from `@x :: TypeName` on struct captures).
    pub fn set_type_name(&mut self, type_id: TypeId, name: Symbol) {
        self.type_names.insert(type_id, name);
    }

    pub fn iter_type_names(&self) -> impl Iterator<Item = (TypeId, Symbol)> + '_ {
        self.type_names.iter().map(|(&id, &sym)| (id, sym))
    }
}
