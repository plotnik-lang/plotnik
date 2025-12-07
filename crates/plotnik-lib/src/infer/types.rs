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
//! ## DefaultQuery Key
//!
//! `TypeKey::DefaultQuery` represents the unnamed entry point query (the last definition
//! without a name). It has no corresponding `TypeValue` variant—it's purely a key that
//! maps to a Struct or other value. The emitted name ("QueryResult" by default) is
//! configurable per code generator.
//!
//! ## Synthetic Keys
//!
//! For nested captures like `(function @fn { (param @p) @params })`, we need unique type
//! names. Synthetic keys use path segments: `["fn", "params"]` → `FnParams`. This avoids
//! name collisions while keeping names readable.

use indexmap::IndexMap;
use rowan::TextRange;

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
    /// The unnamed entry point query (last definition without a name).
    /// Default emitted name is "QueryResult", but emitters may override.
    DefaultQuery,
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
            TypeKey::DefaultQuery => "DefaultQuery".to_string(),
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

    /// Returns true if this is the default query entry point.
    pub fn is_default_query(&self) -> bool {
        matches!(self, TypeKey::DefaultQuery)
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
    /// Source spans where each type was first defined.
    definition_spans: IndexMap<TypeKey<'src>, TextRange>,
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
            definition_spans: IndexMap::new(),
        }
    }

    /// Insert a type definition. Returns the key for chaining.
    pub fn insert(&mut self, key: TypeKey<'src>, value: TypeValue<'src>) -> TypeKey<'src> {
        self.types.insert(key.clone(), value);
        key
    }

    /// Insert a type definition with a source span, detecting conflicts.
    ///
    /// Returns `Ok(key)` if inserted successfully (no conflict).
    /// Returns `Err(existing_span)` if there was an existing incompatible type.
    ///
    /// On conflict, the existing type is NOT overwritten - caller should use Invalid.
    pub fn try_insert(
        &mut self,
        key: TypeKey<'src>,
        value: TypeValue<'src>,
        span: TextRange,
    ) -> Result<TypeKey<'src>, TextRange> {
        if let Some(existing) = self.types.get(&key) {
            if !self.values_are_compatible(existing, &value) {
                let existing_span = self.definition_spans.get(&key).copied().unwrap_or(span);
                return Err(existing_span);
            }
            // Compatible - keep existing, don't overwrite
            return Ok(key);
        }
        self.types.insert(key.clone(), value);
        self.definition_spans.insert(key.clone(), span);
        Ok(key)
    }

    /// Insert without span tracking. Returns true if inserted, false if key existed.
    pub fn try_insert_untracked(&mut self, key: TypeKey<'src>, value: TypeValue<'src>) -> bool {
        if self.types.contains_key(&key) {
            return false;
        }
        self.types.insert(key, value);
        true
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

    /// Check if two type keys are structurally compatible.
    ///
    /// For built-in types, this is simple equality.
    /// For synthetic types, we compare the underlying TypeValue structure.
    /// Two synthetic keys pointing to different TaggedUnions or Structs are incompatible.
    pub fn types_are_compatible(&self, a: &TypeKey<'src>, b: &TypeKey<'src>) -> bool {
        if a == b {
            return true;
        }

        // Invalid is compatible with anything - don't cascade errors
        if *a == TypeKey::Invalid || *b == TypeKey::Invalid {
            return true;
        }

        // Different built-in types are incompatible
        if a.is_builtin() || b.is_builtin() {
            return false;
        }

        // For synthetic/named types, compare the underlying values
        let val_a = self.get(a);
        let val_b = self.get(b);

        match (val_a, val_b) {
            (Some(va), Some(vb)) => self.values_are_compatible(va, vb),
            // If either is missing, consider incompatible (shouldn't happen in practice)
            _ => false,
        }
    }

    /// Check if two type values are structurally compatible.
    fn values_are_compatible(&self, a: &TypeValue<'src>, b: &TypeValue<'src>) -> bool {
        use TypeValue::*;
        match (a, b) {
            (Node, Node) => true,
            (String, String) => true,
            (Unit, Unit) => true,
            (Invalid, Invalid) => true,
            (Optional(ka), Optional(kb)) => self.types_are_compatible(ka, kb),
            (List(ka), List(kb)) => self.types_are_compatible(ka, kb),
            (NonEmptyList(ka), NonEmptyList(kb)) => self.types_are_compatible(ka, kb),
            // List and NonEmptyList are compatible if inner types match - merge to List
            (List(ka), NonEmptyList(kb)) | (NonEmptyList(ka), List(kb)) => {
                self.types_are_compatible(ka, kb)
            }
            (Struct(fa), Struct(fb)) => {
                // Structs must have exactly the same fields with compatible types
                if fa.len() != fb.len() {
                    return false;
                }
                for (name, key_a) in fa {
                    match fb.get(name) {
                        Some(key_b) => {
                            if !self.types_are_compatible(key_a, key_b) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (TaggedUnion(va), TaggedUnion(vb)) => {
                // TaggedUnions must have exactly the same variants
                if va.len() != vb.len() {
                    return false;
                }
                for (name, key_a) in va {
                    match vb.get(name) {
                        Some(key_b) => {
                            if !self.types_are_compatible(key_a, key_b) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            // Different type constructors are incompatible
            _ => false,
        }
    }

    /// Iterate over all types in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&TypeKey<'src>, &TypeValue<'src>)> {
        self.types.iter()
    }

    /// Try to merge List and NonEmptyList types into List.
    ///
    /// Returns `Some(List(inner))` if one is List and other is NonEmptyList with compatible inner types.
    /// Returns `None` otherwise.
    fn try_merge_list_types(
        &mut self,
        a: &TypeKey<'src>,
        b: &TypeKey<'src>,
    ) -> Option<TypeKey<'src>> {
        let val_a = self.get(a)?;
        let val_b = self.get(b)?;

        let inner = match (val_a, val_b) {
            (TypeValue::List(ka), TypeValue::NonEmptyList(kb))
            | (TypeValue::NonEmptyList(ka), TypeValue::List(kb)) => {
                if self.types_are_compatible(ka, kb) {
                    ka.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        // Return or create a List type with the inner type
        let list_key = TypeKey::Synthetic(vec!["list", "merged"]);
        self.insert(list_key.clone(), TypeValue::List(inner));
        Some(list_key)
    }

    /// Try to merge two struct types into one, returning the merged fields.
    ///
    /// Returns `Some(merged_fields)` if both types are structs (regardless of field shape).
    /// Returns `None` if either type is not a struct.
    ///
    /// The merge rules:
    /// - Fields present in both structs with compatible types keep that type
    /// - Fields present in only one struct become Optional
    /// - Fields with conflicting types become Invalid
    fn try_merge_struct_fields(
        &self,
        a: &TypeKey<'src>,
        b: &TypeKey<'src>,
    ) -> Option<IndexMap<&'src str, MergedField<'src>>> {
        let val_a = self.get(a)?;
        let val_b = self.get(b)?;

        let (fields_a, fields_b) = match (val_a, val_b) {
            (TypeValue::Struct(fa), TypeValue::Struct(fb)) => (fa, fb),
            _ => return None,
        };

        // Collect all field names from both structs
        let mut all_fields: IndexMap<&'src str, ()> = IndexMap::new();
        for name in fields_a.keys() {
            all_fields.entry(*name).or_insert(());
        }
        for name in fields_b.keys() {
            all_fields.entry(*name).or_insert(());
        }

        let mut result = IndexMap::new();
        for field_name in all_fields.keys() {
            let type_a = fields_a.get(field_name);
            let type_b = fields_b.get(field_name);

            let merged = match (type_a, type_b) {
                (Some(ta), Some(tb)) => {
                    if self.types_are_compatible(ta, tb) {
                        MergedField::Same(ta.clone())
                    } else {
                        // Recursively try to merge nested structs
                        if let Some(nested_merged) = self.try_merge_struct_fields(ta, tb) {
                            if nested_merged
                                .values()
                                .any(|m| matches!(m, MergedField::Conflict))
                            {
                                MergedField::Conflict
                            } else {
                                // Both are structs - they can be merged (caller handles actual merge)
                                MergedField::Same(ta.clone())
                            }
                        } else {
                            MergedField::Conflict
                        }
                    }
                }
                (Some(t), None) | (None, Some(t)) => MergedField::Optional(t.clone()),
                (None, None) => continue,
            };
            result.insert(*field_name, merged);
        }

        Some(result)
    }

    /// Merge fields from multiple struct branches (for untagged unions).
    ///
    /// Given a list of field maps (one per branch), produces a merged field map where:
    /// - Fields present in all branches with the same type keep that type
    /// - Fields present in only some branches become Optional
    /// - Fields with conflicting types across branches become Invalid
    /// - Fields that are both structs get recursively merged
    ///
    /// # Example
    ///
    /// Branch 1: `{ name: String, value: Node }`
    /// Branch 2: `{ name: String, extra: Node }`
    ///
    /// Merged: `{ name: String, value: Optional<Node>, extra: Optional<Node> }`
    ///
    /// # Struct Merge Example
    ///
    /// Branch 1: `{ x: { y: Node } }`
    /// Branch 2: `{ x: { z: Node } }`
    ///
    /// Merged: `{ x: { y: Optional<Node>, z: Optional<Node> } }`
    ///
    /// # Type Conflict Example
    ///
    /// Branch 1: `{ x: String }`
    /// Branch 2: `{ x: Node }`
    ///
    /// Merged: `{ x: Invalid }` (with diagnostic warning)
    pub fn merge_fields(
        &mut self,
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

            // Check if all occurrences have compatible types (structural comparison)
            let first_type = type_occurrences[0];
            let all_same_type = type_occurrences
                .iter()
                .all(|t| self.types_are_compatible(t, first_type));

            // Check for List/NonEmptyList merge case
            let list_merge_key = if type_occurrences.len() == 2 {
                self.try_merge_list_types(type_occurrences[0], type_occurrences[1])
            } else {
                None
            };

            let merged = if let Some(merged_key) = list_merge_key {
                // List and NonEmptyList merged to List
                if present_count == branch_count {
                    MergedField::Same(merged_key)
                } else {
                    MergedField::Optional(merged_key)
                }
            } else if !all_same_type {
                // Types differ - try to merge if both are structs
                if type_occurrences.len() == 2 {
                    if let Some(struct_merged) =
                        self.try_merge_struct_fields(type_occurrences[0], type_occurrences[1])
                    {
                        // Both are structs - create a merged struct type
                        let merged_fields: IndexMap<&'src str, TypeKey<'src>> = struct_merged
                            .into_iter()
                            .map(|(name, mf)| {
                                let key = match mf {
                                    MergedField::Same(k) => k,
                                    MergedField::Optional(k) => {
                                        let wrapper_key =
                                            TypeKey::Synthetic(vec![*field_name, name, "opt"]);
                                        self.insert(wrapper_key.clone(), TypeValue::Optional(k));
                                        wrapper_key
                                    }
                                    MergedField::Conflict => TypeKey::Invalid,
                                };
                                (name, key)
                            })
                            .collect();

                        // Create a new merged struct type
                        let merged_key = TypeKey::Synthetic(vec![*field_name, "merged"]);
                        self.insert(merged_key.clone(), TypeValue::Struct(merged_fields));

                        if present_count == branch_count {
                            MergedField::Same(merged_key)
                        } else {
                            MergedField::Optional(merged_key)
                        }
                    } else {
                        MergedField::Conflict
                    }
                } else {
                    // More than 2 branches with different struct types - TODO: support N-way merge
                    MergedField::Conflict
                }
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
