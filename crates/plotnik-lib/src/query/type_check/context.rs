//! TypeContext: manages interned types, symbols, and term info cache.
//!
//! Types are interned to enable cheap equality checks and cycle handling.
//! Symbols are interned to enable cheap string comparison.
//! TermInfo is cached per-expression to avoid recomputation.

use std::collections::{BTreeMap, HashMap};

use crate::parser::ast::Expr;

use super::symbol::{DefId, Interner, Symbol};
use super::types::{
    Arity, FieldInfo, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeId, TypeKind,
};

/// Central registry for types, symbols, and expression metadata.
#[derive(Debug, Clone)]
pub struct TypeContext {
    /// String interner for field/type names
    interner: Interner,
    /// Interned types by ID
    types: Vec<TypeKind>,
    /// Deduplication map for type interning
    type_map: HashMap<TypeKind, TypeId>,
    /// Cached term info per expression
    term_info: HashMap<Expr, TermInfo>,
    /// Definition-level type info (for TypeScript emission), keyed by DefId
    def_types: HashMap<DefId, TypeId>,
    /// DefId → Symbol mapping (for resolving def names)
    def_names: Vec<Symbol>,
    /// Symbol → DefId reverse lookup
    def_ids: HashMap<Symbol, DefId>,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    pub fn new() -> Self {
        let mut ctx = Self {
            interner: Interner::new(),
            types: Vec::new(),
            type_map: HashMap::new(),
            term_info: HashMap::new(),
            def_types: HashMap::new(),
            def_names: Vec::new(),
            def_ids: HashMap::new(),
        };

        // Pre-register builtin types at their expected IDs
        let void_id = ctx.intern_type(TypeKind::Void);
        debug_assert_eq!(void_id, TYPE_VOID);

        let node_id = ctx.intern_type(TypeKind::Node);
        debug_assert_eq!(node_id, TYPE_NODE);

        let string_id = ctx.intern_type(TypeKind::String);
        debug_assert_eq!(string_id, TYPE_STRING);

        ctx
    }

    // ========== Symbol interning ==========

    /// Intern a string, returning its Symbol.
    #[inline]
    pub fn intern(&mut self, s: &str) -> Symbol {
        self.interner.intern(s)
    }

    /// Intern an owned string.
    #[inline]
    pub fn intern_owned(&mut self, s: String) -> Symbol {
        self.interner.intern_owned(s)
    }

    /// Resolve a Symbol back to its string.
    #[inline]
    pub fn resolve(&self, sym: Symbol) -> &str {
        self.interner.resolve(sym)
    }

    /// Get a reference to the interner (for emission, etc.).
    #[inline]
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    // ========== Type interning ==========

    /// Intern a type, returning its ID. Deduplicates identical types.
    pub fn intern_type(&mut self, kind: TypeKind) -> TypeId {
        if let Some(&id) = self.type_map.get(&kind) {
            return id;
        }

        let id = TypeId(self.types.len() as u32);
        self.types.push(kind.clone());
        self.type_map.insert(kind, id);
        id
    }

    /// Get the TypeKind for a TypeId.
    pub fn get_type(&self, id: TypeId) -> Option<&TypeKind> {
        self.types.get(id.0 as usize)
    }

    /// Get or create a type, returning both the ID and a reference.
    pub fn get_or_intern(&mut self, kind: TypeKind) -> (TypeId, &TypeKind) {
        let id = self.intern_type(kind);
        (id, &self.types[id.0 as usize])
    }

    /// Intern a struct type from fields.
    pub fn intern_struct(&mut self, fields: BTreeMap<Symbol, FieldInfo>) -> TypeId {
        self.intern_type(TypeKind::Struct(fields))
    }

    /// Intern a struct type with a single field.
    pub fn intern_single_field(&mut self, name: Symbol, info: FieldInfo) -> TypeId {
        let mut fields = BTreeMap::new();
        fields.insert(name, info);
        self.intern_type(TypeKind::Struct(fields))
    }

    /// Get struct fields from a TypeId, if it points to a Struct.
    pub fn get_struct_fields(&self, id: TypeId) -> Option<&BTreeMap<Symbol, FieldInfo>> {
        match self.get_type(id)? {
            TypeKind::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    // ========== Term info cache ==========

    /// Cache term info for an expression.
    pub fn set_term_info(&mut self, expr: Expr, info: TermInfo) {
        self.term_info.insert(expr, info);
    }

    /// Get cached term info for an expression.
    pub fn get_term_info(&self, expr: &Expr) -> Option<&TermInfo> {
        self.term_info.get(expr)
    }

    // ========== Definition registry ==========

    /// Register a definition by name, returning its DefId.
    /// If already registered, returns existing DefId.
    pub fn register_def(&mut self, name: &str) -> DefId {
        let sym = self.interner.intern(name);
        if let Some(&def_id) = self.def_ids.get(&sym) {
            return def_id;
        }
        let def_id = DefId::from_raw(self.def_names.len() as u32);
        self.def_names.push(sym);
        self.def_ids.insert(sym, def_id);
        def_id
    }

    /// Get DefId for a definition name.
    pub fn get_def_id(&self, name: &str) -> Option<DefId> {
        // Need to check if interned, avoid creating new symbol
        for (&sym, &def_id) in &self.def_ids {
            if self.interner.resolve(sym) == name {
                return Some(def_id);
            }
        }
        None
    }

    /// Get the name Symbol for a DefId.
    pub fn def_name_sym(&self, def_id: DefId) -> Symbol {
        self.def_names[def_id.index()]
    }

    /// Get the name string for a DefId.
    pub fn def_name(&self, def_id: DefId) -> &str {
        self.resolve(self.def_names[def_id.index()])
    }

    // ========== Definition types ==========

    /// Register the output type for a definition by DefId.
    pub fn set_def_type(&mut self, def_id: DefId, type_id: TypeId) {
        self.def_types.insert(def_id, type_id);
    }

    /// Register the output type for a definition by string name.
    /// Registers the def if not already known.
    pub fn set_def_type_by_name(&mut self, name: &str, type_id: TypeId) {
        let def_id = self.register_def(name);
        self.def_types.insert(def_id, type_id);
    }

    /// Get the output type for a definition by DefId.
    pub fn get_def_type(&self, def_id: DefId) -> Option<TypeId> {
        self.def_types.get(&def_id).copied()
    }

    /// Get the output type for a definition by string name.
    pub fn get_def_type_by_name(&self, name: &str) -> Option<TypeId> {
        self.get_def_id(name)
            .and_then(|id| self.def_types.get(&id).copied())
    }

    /// Get arity for an expression (for backward compatibility with expr_arity).
    pub fn get_arity(&self, expr: &Expr) -> Option<Arity> {
        self.term_info.get(expr).map(|info| info.arity)
    }

    // ========== Iteration ==========

    /// Iterate over all interned types.
    pub fn iter_types(&self) -> impl Iterator<Item = (TypeId, &TypeKind)> {
        self.types
            .iter()
            .enumerate()
            .map(|(i, k)| (TypeId(i as u32), k))
    }

    /// Number of interned types.
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Iterate over all definition types as (DefId, TypeId).
    pub fn iter_def_types(&self) -> impl Iterator<Item = (DefId, TypeId)> + '_ {
        self.def_types
            .iter()
            .map(|(&def_id, &type_id)| (def_id, type_id))
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

        assert_eq!(ctx.get_type(TYPE_VOID), Some(&TypeKind::Void));
        assert_eq!(ctx.get_type(TYPE_NODE), Some(&TypeKind::Node));
        assert_eq!(ctx.get_type(TYPE_STRING), Some(&TypeKind::String));
    }

    #[test]
    fn type_interning_deduplicates() {
        let mut ctx = TypeContext::new();

        let id1 = ctx.intern_type(TypeKind::Node);
        let id2 = ctx.intern_type(TypeKind::Node);

        assert_eq!(id1, id2);
        assert_eq!(id1, TYPE_NODE);
    }

    #[test]
    fn struct_types_intern_correctly() {
        let mut ctx = TypeContext::new();

        let x_sym = ctx.intern("x");
        let mut fields = BTreeMap::new();
        fields.insert(x_sym, FieldInfo::required(TYPE_NODE));

        let id1 = ctx.intern_type(TypeKind::Struct(fields.clone()));
        let id2 = ctx.intern_type(TypeKind::Struct(fields));

        assert_eq!(id1, id2);
    }

    #[test]
    fn symbol_interning_works() {
        let mut ctx = TypeContext::new();

        let a = ctx.intern("foo");
        let b = ctx.intern("foo");
        let c = ctx.intern("bar");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(ctx.resolve(a), "foo");
        assert_eq!(ctx.resolve(c), "bar");
    }

    #[test]
    fn def_type_by_name() {
        let mut ctx = TypeContext::new();

        ctx.set_def_type_by_name("Query", TYPE_NODE);
        assert_eq!(ctx.get_def_type_by_name("Query"), Some(TYPE_NODE));
        assert_eq!(ctx.get_def_type_by_name("Missing"), None);
    }

    #[test]
    fn register_def_returns_stable_id() {
        let mut ctx = TypeContext::new();

        let id1 = ctx.register_def("Foo");
        let id2 = ctx.register_def("Bar");
        let id3 = ctx.register_def("Foo"); // duplicate

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(ctx.def_name(id1), "Foo");
        assert_eq!(ctx.def_name(id2), "Bar");
    }

    #[test]
    fn def_id_lookup() {
        let mut ctx = TypeContext::new();

        ctx.register_def("Query");
        assert!(ctx.get_def_id("Query").is_some());
        assert!(ctx.get_def_id("Missing").is_none());
    }
}
