//! TypeContext: manages interned types, symbols, and term info cache.
//!
//! Types are interned to enable cheap equality checks and cycle handling.
//! Symbols are interned to enable cheap string comparison.
//! TermInfo is cached per-expression to avoid recomputation.

use std::collections::HashMap;

use crate::parser::ast::Expr;

use super::symbol::{Interner, Symbol};
use super::types::{Arity, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeId, TypeKind};

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
    /// Definition-level type info (for TypeScript emission)
    def_types: HashMap<Symbol, TypeId>,
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

    // ========== Term info cache ==========

    /// Cache term info for an expression.
    pub fn set_term_info(&mut self, expr: Expr, info: TermInfo) {
        self.term_info.insert(expr, info);
    }

    /// Get cached term info for an expression.
    pub fn get_term_info(&self, expr: &Expr) -> Option<&TermInfo> {
        self.term_info.get(expr)
    }

    // ========== Definition types ==========

    /// Register the output type for a definition.
    pub fn set_def_type(&mut self, name: Symbol, type_id: TypeId) {
        self.def_types.insert(name, type_id);
    }

    /// Register the output type for a definition by string name.
    pub fn set_def_type_by_name(&mut self, name: &str, type_id: TypeId) {
        let sym = self.interner.intern(name);
        self.def_types.insert(sym, type_id);
    }

    /// Get the output type for a definition.
    pub fn get_def_type(&self, name: Symbol) -> Option<TypeId> {
        self.def_types.get(&name).copied()
    }

    /// Get the output type for a definition by string name.
    pub fn get_def_type_by_name(&self, name: &str) -> Option<TypeId> {
        // Linear scan since we don't have reverse lookup without interning
        for (&sym, &type_id) in &self.def_types {
            if self.interner.resolve(sym) == name {
                return Some(type_id);
            }
        }
        None
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

    /// Iterate over all definition types.
    pub fn iter_def_types(&self) -> impl Iterator<Item = (Symbol, TypeId)> + '_ {
        self.def_types.iter().map(|(&sym, &type_id)| (sym, type_id))
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
}
