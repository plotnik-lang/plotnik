//! TypeContext: manages interned types, symbols, and term info cache.
//!
//! Types are interned to enable cheap equality checks and cycle handling.
//! Symbols are stored but resolved via external Interner reference.
//! TermInfo is cached per-expression to avoid recomputation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::parser::ast::Expr;

use super::symbol::{DefId, Interner, Symbol};
use super::types::{
    Arity, FieldInfo, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeId, TypeShape,
};

/// Central registry for types, symbols, and expression metadata.
#[derive(Clone, Debug)]
pub struct TypeContext {
    types: Vec<TypeShape>,
    type_map: HashMap<TypeShape, TypeId>,

    def_names: Vec<Symbol>,
    def_ids: HashMap<Symbol, DefId>,
    /// Definition-level type info (for TypeScript emission), keyed by DefId
    def_types: HashMap<DefId, TypeId>,
    /// Definitions that are part of a recursive SCC
    recursive_defs: HashSet<DefId>,

    term_info: HashMap<Expr, TermInfo>,
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
            recursive_defs: HashSet::new(),
            term_info: HashMap::new(),
        };

        // Pre-register builtin types at their expected IDs
        let void_id = ctx.intern_type(TypeShape::Void);
        debug_assert_eq!(void_id, TYPE_VOID);

        let node_id = ctx.intern_type(TypeShape::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        let string_id = ctx.intern_type(TypeShape::String);
        debug_assert_eq!(string_id, TYPE_STRING);

        ctx
    }

    /// Seed definition mappings from DependencyAnalysis.
    /// This avoids re-registering definitions that were already assigned DefIds.
    pub fn seed_defs(&mut self, def_names: &[Symbol], name_to_def: &HashMap<Symbol, DefId>) {
        self.def_names = def_names.to_vec();
        self.def_ids = name_to_def.clone();
    }

    /// Intern a type, returning its ID. Deduplicates identical types.
    pub fn intern_type(&mut self, shape: TypeShape) -> TypeId {
        if let Some(&id) = self.type_map.get(&shape) {
            return id;
        }

        let id = TypeId(self.types.len() as u32);
        self.types.push(shape.clone());
        self.type_map.insert(shape, id);
        id
    }

    /// Get the TypeShape for a TypeId.
    pub fn get_type(&self, id: TypeId) -> Option<&TypeShape> {
        self.types.get(id.0 as usize)
    }

    /// Get or create a type, returning both the ID and a reference.
    pub fn get_or_intern(&mut self, shape: TypeShape) -> (TypeId, &TypeShape) {
        let id = self.intern_type(shape);
        (id, &self.types[id.0 as usize])
    }

    /// Intern a struct type from fields.
    pub fn intern_struct(&mut self, fields: BTreeMap<Symbol, FieldInfo>) -> TypeId {
        self.intern_type(TypeShape::Struct(fields))
    }

    /// Intern a struct type with a single field.
    pub fn intern_single_field(&mut self, name: Symbol, info: FieldInfo) -> TypeId {
        self.intern_type(TypeShape::Struct(BTreeMap::from([(name, info)])))
    }

    /// Get struct fields from a TypeId, if it points to a Struct.
    pub fn get_struct_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, FieldInfo>> {
        match self.get_type(id)? {
            TypeShape::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Cache term info for an expression.
    pub fn set_term_info(&mut self, expr: Expr, info: TermInfo) {
        self.term_info.insert(expr, info);
    }

    /// Get cached term info for an expression.
    pub fn get_term_info(&self, expr: &Expr) -> Option<&TermInfo> {
        self.term_info.get(expr)
    }

    /// Register a definition by name, returning its DefId.
    pub fn register_def(&mut self, interner: &mut Interner, name: &str) -> DefId {
        let sym = interner.intern(name);
        self.register_def_sym(sym)
    }

    /// Register a definition by pre-interned Symbol, returning its DefId.
    pub fn register_def_sym(&mut self, sym: Symbol) -> DefId {
        if let Some(&def_id) = self.def_ids.get(&sym) {
            return def_id;
        }

        let def_id = DefId::from_raw(self.def_names.len() as u32);
        self.def_names.push(sym);
        self.def_ids.insert(sym, def_id);
        def_id
    }

    /// Get DefId for a definition by Symbol.
    pub fn get_def_id_sym(&self, sym: Symbol) -> Option<DefId> {
        self.def_ids.get(&sym).copied()
    }

    /// Get DefId for a definition name (requires interner for lookup).
    pub fn get_def_id(&self, interner: &Interner, name: &str) -> Option<DefId> {
        // Linear scan - only used during analysis, not hot path.
        // Necessary because we don't assume Interner has reverse lookup here.
        self.def_ids
            .iter()
            .find_map(|(&sym, &id)| (interner.resolve(sym) == name).then_some(id))
    }

    /// Get the name Symbol for a DefId.
    pub fn def_name_sym(&self, def_id: DefId) -> Symbol {
        self.def_names[def_id.index()]
    }

    /// Get the name string for a DefId.
    pub fn def_name<'a>(&self, interner: &'a Interner, def_id: DefId) -> &'a str {
        interner.resolve(self.def_names[def_id.index()])
    }

    /// Mark a definition as recursive.
    pub fn mark_recursive(&mut self, def_id: DefId) {
        self.recursive_defs.insert(def_id);
    }

    /// Check if a definition is recursive.
    pub fn is_recursive(&self, def_id: DefId) -> bool {
        self.recursive_defs.contains(&def_id)
    }

    /// Register the output type for a definition by DefId.
    pub fn set_def_type(&mut self, def_id: DefId, type_id: TypeId) {
        self.def_types.insert(def_id, type_id);
    }

    /// Register the output type for a definition by string name.
    /// Registers the def if not already known.
    pub fn set_def_type_by_name(&mut self, interner: &mut Interner, name: &str, type_id: TypeId) {
        let def_id = self.register_def(interner, name);
        self.set_def_type(def_id, type_id);
    }

    /// Get the output type for a definition by DefId.
    pub fn get_def_type(&self, def_id: DefId) -> Option<TypeId> {
        self.def_types.get(&def_id).copied()
    }

    /// Get the output type for a definition by string name.
    pub fn get_def_type_by_name(&self, interner: &Interner, name: &str) -> Option<TypeId> {
        let id = self.get_def_id(interner, name)?;
        self.get_def_type(id)
    }

    /// Get arity for an expression.
    pub fn get_arity(&self, expr: &Expr) -> Option<Arity> {
        self.term_info.get(expr).map(|info| info.arity)
    }

    /// Iterate over all interned types.
    pub fn iter_types(&self) -> impl Iterator<Item = (TypeId, &TypeShape)> {
        self.types
            .iter()
            .enumerate()
            .map(|(i, k)| (TypeId(i as u32), k))
    }

    /// Number of interned types.
    pub fn type_count(&self) -> usize {
        self.types.len()
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
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::query::type_check::types::FieldInfo;

    #[test]
    fn builtin_types_have_correct_ids() {
        let ctx = TypeContext::new();

        assert_eq!(ctx.get_type(TYPE_VOID), Some(&TypeShape::Void));
        assert_eq!(ctx.get_type(TYPE_NODE), Some(&TypeShape::Node));
        assert_eq!(ctx.get_type(TYPE_STRING), Some(&TypeShape::String));
    }

    #[test]
    fn type_interning_deduplicates() {
        let mut ctx = TypeContext::new();

        let id1 = ctx.intern_type(TypeShape::Node);
        let id2 = ctx.intern_type(TypeShape::Node);

        assert_eq!(id1, id2);
        assert_eq!(id1, TYPE_NODE);
    }

    #[test]
    fn struct_types_intern_correctly() {
        let mut ctx = TypeContext::new();
        let mut interner = Interner::new();

        let x_sym = interner.intern("x");
        let mut fields = BTreeMap::new();
        fields.insert(x_sym, FieldInfo::required(TYPE_NODE));

        let id1 = ctx.intern_type(TypeShape::Struct(fields.clone()));
        let id2 = ctx.intern_type(TypeShape::Struct(fields));

        assert_eq!(id1, id2);
    }

    #[test]
    fn symbol_interning_works() {
        let mut interner = Interner::new();

        let a = interner.intern("foo");
        let b = interner.intern("foo");
        let c = interner.intern("bar");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(interner.resolve(a), "foo");
        assert_eq!(interner.resolve(c), "bar");
    }

    #[test]
    fn def_type_by_name() {
        let mut ctx = TypeContext::new();
        let mut interner = Interner::new();

        ctx.set_def_type_by_name(&mut interner, "Query", TYPE_NODE);
        assert_eq!(
            ctx.get_def_type_by_name(&interner, "Query"),
            Some(TYPE_NODE)
        );
        assert_eq!(ctx.get_def_type_by_name(&interner, "Missing"), None);
    }

    #[test]
    fn register_def_returns_stable_id() {
        let mut ctx = TypeContext::new();
        let mut interner = Interner::new();

        let id1 = ctx.register_def(&mut interner, "Foo");
        let id2 = ctx.register_def(&mut interner, "Bar");
        let id3 = ctx.register_def(&mut interner, "Foo"); // duplicate

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(ctx.def_name(&interner, id1), "Foo");
        assert_eq!(ctx.def_name(&interner, id2), "Bar");
    }

    #[test]
    fn def_id_lookup() {
        let mut ctx = TypeContext::new();
        let mut interner = Interner::new();

        ctx.register_def(&mut interner, "Query");
        assert!(ctx.get_def_id(&interner, "Query").is_some());
        assert!(ctx.get_def_id(&interner, "Missing").is_none());
    }
}
