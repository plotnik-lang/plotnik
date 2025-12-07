//! Type representation for inferred query output types.
//!
//! # Overview
//!
//! The type system is flat: all types live in a `TypeTable` keyed by `TypeKey`.
//! Wrapper types (Optional, List, NonEmptyList) reference inner types by key.
//!
//! # Design Decisions
//!
//! ## Alternation Handling
//!
//! Alternations (`[A: ... B: ...]` or `[... ...]`) produce different type structures:
//!
//! - **Tagged alternations** (`[A: expr B: expr]`): Become `TaggedUnion` with named variants.
//!   Each branch gets its own struct type, discriminated by the tag name.
//!
//! - **Untagged/mixed alternations** (`[expr expr]`): Branches are "merged" into a single
//!   struct where fields are combined. The merge rules:
//!   1. Field present in all branches with same type → field has that type
//!   2. Field present in some branches only → field becomes Optional
//!   3. Field present in all branches but with different types → field gets Invalid type
//!
//! ## Invalid Type
//!
//! The `Invalid` type represents a type conflict that couldn't be resolved (e.g., field
//! has `Node` in one branch and `String` in another). It is emitted the same as `Unit`
//! in code generators—this keeps output valid while signaling the user made a questionable
//! query. Diagnostics should warn about Invalid types during inference.
//!
//! ## Type Keys vs Type Values
//!
//! - `TypeKey`: Identity/reference to a type. Used in field types, wrapper inner types.
//! - `TypeValue`: The actual type definition. Stored in the table.
//!
//! Built-in types (Node, String, Unit, Invalid) have both a key and value variant for
//! consistency—the key is what you reference, the value is what gets stored.
//!
//! ## Synthetic Keys
//!
//! For nested captures like `(function @fn { (param @p) @params })`, we need unique type
//! names. Synthetic keys use path segments: `["fn", "params"]` → `FnParams`. This avoids
//! name collisions while keeping names readable.

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
    /// Invalid type for unresolvable conflicts (built-in)
    /// Emitted same as Unit in code generators.
    Invalid,
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
            TypeKey::Invalid => "Unit".to_string(), // Invalid emits as Unit
            TypeKey::Named(name) => (*name).to_string(),
            TypeKey::Synthetic(segments) => segments.iter().map(|s| to_pascal(s)).collect(),
        }
    }

    /// Returns true if this is a built-in primitive type.
    pub fn is_builtin(&self) -> bool {
        matches!(
            self,
            TypeKey::Node | TypeKey::String | TypeKey::Unit | TypeKey::Invalid
        )
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
    /// Invalid type (conflicting types in untagged union)
    /// Emitted same as Unit. Presence indicates a diagnostic should be emitted.
    Invalid,
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

/// Result of merging a single field across branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergedField<'src> {
    /// Field has same type in all branches where present
    Same(TypeKey<'src>),
    /// Field has same type but missing in some branches → needs Optional wrapper
    Optional(TypeKey<'src>),
    /// Field has conflicting types across branches → Invalid
    Conflict,
}

/// Collection of all inferred types for a query.
#[derive(Debug, Clone)]
pub struct TypeTable<'src> {
    /// All type definitions, keyed by their identity.
    /// Pre-populated with built-in types (Node, String, Unit, Invalid).
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
        types.insert(TypeKey::Invalid, TypeValue::Invalid);
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

    /// Merge fields from multiple struct branches (for untagged unions).
    ///
    /// Given a list of field maps (one per branch), produces a merged field map where:
    /// - Fields present in all branches with the same type keep that type
    /// - Fields present in only some branches become Optional
    /// - Fields with conflicting types across branches become Invalid
    ///
    /// # Example
    ///
    /// Branch 1: `{ name: String, value: Node }`
    /// Branch 2: `{ name: String, extra: Node }`
    ///
    /// Merged: `{ name: String, value: Optional<Node>, extra: Optional<Node> }`
    ///
    /// # Type Conflict Example
    ///
    /// Branch 1: `{ x: String }`
    /// Branch 2: `{ x: Node }`
    ///
    /// Merged: `{ x: Invalid }` (with diagnostic warning)
    pub fn merge_fields(
        branches: &[IndexMap<&'src str, TypeKey<'src>>],
    ) -> IndexMap<&'src str, MergedField<'src>> {
        if branches.is_empty() {
            return IndexMap::new();
        }

        // Collect all field names across all branches
        let mut all_fields: IndexMap<&'src str, ()> = IndexMap::new();
        for branch in branches {
            for field_name in branch.keys() {
                all_fields.entry(*field_name).or_insert(());
            }
        }

        let mut result = IndexMap::new();
        let branch_count = branches.len();

        for field_name in all_fields.keys() {
            // Collect (type, count) for this field across branches
            let mut type_occurrences: Vec<&TypeKey<'src>> = Vec::new();
            for branch in branches {
                if let Some(ty) = branch.get(field_name) {
                    type_occurrences.push(ty);
                }
            }

            let present_count = type_occurrences.len();
            if present_count == 0 {
                continue;
            }

            // Check if all occurrences have the same type
            let first_type = type_occurrences[0];
            let all_same_type = type_occurrences.iter().all(|t| *t == first_type);

            let merged = if !all_same_type {
                // Type conflict
                MergedField::Conflict
            } else if present_count == branch_count {
                // Present in all branches with same type
                MergedField::Same(first_type.clone())
            } else {
                // Present in some branches only
                MergedField::Optional(first_type.clone())
            };

            result.insert(*field_name, merged);
        }

        result
    }
}

impl Default for TypeTable<'_> {
    fn default() -> Self {
        Self::new()
    }
}
