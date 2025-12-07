//! Type representation for inferred query output types.
//!
//! The type system is flat: all types live in a `TypeTable` keyed by `TypeKey`.
//! Wrapper types (Optional, List, NonEmptyList) reference inner types by key.

use indexmap::IndexMap;

/// Identity of a type in the type table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKey<'src> {
    /// Tree-sitter node (built-in)
    Node,
    /// String value from `:: string` annotation (built-in)
    String,
    /// Unit type for empty captures (built-in)
    Unit,
    /// User-provided type name via `:: TypeName`
    Named(&'src str),
    /// Path-based synthetic name: ["Foo", "bar"] → FooBar
    Synthetic(Vec<&'src str>),
}

impl TypeKey<'_> {
    /// Render as PascalCase type name.
    pub fn to_pascal_case(&self) -> String {
        match self {
            TypeKey::Node => "Node".to_string(),
            TypeKey::String => "String".to_string(),
            TypeKey::Unit => "Unit".to_string(),
            TypeKey::Named(name) => (*name).to_string(),
            TypeKey::Synthetic(segments) => segments.iter().map(|s| to_pascal(s)).collect(),
        }
    }
}

/// Convert snake_case or lowercase to PascalCase.
pub(crate) fn to_pascal(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Type definition stored in the type table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeValue<'src> {
    /// Tree-sitter node primitive
    Node,
    /// String primitive
    String,
    /// Unit type (empty struct)
    Unit,
    /// Struct with named fields
    Struct(IndexMap<&'src str, TypeKey<'src>>),
    /// Tagged union: variant name → variant type (must resolve to Struct or Unit)
    TaggedUnion(IndexMap<&'src str, TypeKey<'src>>),
    /// Optional wrapper
    Optional(TypeKey<'src>),
    /// Zero-or-more list wrapper
    List(TypeKey<'src>),
    /// One-or-more list wrapper
    NonEmptyList(TypeKey<'src>),
}

/// Collection of all inferred types for a query.
#[derive(Debug, Clone)]
pub struct TypeTable<'src> {
    /// All type definitions, keyed by their identity.
    /// Pre-populated with built-in types (Node, String, Unit).
    pub types: IndexMap<TypeKey<'src>, TypeValue<'src>>,
    /// Types that contain cyclic references (need Box in Rust).
    pub cyclic: Vec<TypeKey<'src>>,
}

impl<'src> TypeTable<'src> {
    /// Create a new type table with built-in types pre-populated.
    pub fn new() -> Self {
        let mut types = IndexMap::new();
        types.insert(TypeKey::Node, TypeValue::Node);
        types.insert(TypeKey::String, TypeValue::String);
        types.insert(TypeKey::Unit, TypeValue::Unit);
        Self {
            types,
            cyclic: Vec::new(),
        }
    }

    /// Insert a type definition. Returns the key for chaining.
    pub fn insert(&mut self, key: TypeKey<'src>, value: TypeValue<'src>) -> TypeKey<'src> {
        self.types.insert(key.clone(), value);
        key
    }

    /// Mark a type as cyclic (requires indirection in Rust).
    pub fn mark_cyclic(&mut self, key: TypeKey<'src>) {
        if !self.cyclic.contains(&key) {
            self.cyclic.push(key);
        }
    }

    /// Check if a type is cyclic.
    pub fn is_cyclic(&self, key: &TypeKey<'src>) -> bool {
        self.cyclic.contains(key)
    }

    /// Get a type by key.
    pub fn get(&self, key: &TypeKey<'src>) -> Option<&TypeValue<'src>> {
        self.types.get(key)
    }

    /// Iterate over all types in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&TypeKey<'src>, &TypeValue<'src>)> {
        self.types.iter()
    }
}

impl Default for TypeTable<'_> {
    fn default() -> Self {
        Self::new()
    }
}
