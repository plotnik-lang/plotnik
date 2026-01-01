//! Bytecode emission from compiled queries.
//!
//! Converts the compiled IR into the binary bytecode format. This module handles:
//! - String table construction and interning
//! - Type table building with field resolution
//! - Cache-aligned instruction layout
//! - Section assembly and header generation
//!
//! Entry points:
//! - [`emit`]: Emit bytecode without language linking
//! - [`emit_linked`]: Emit bytecode with node type/field validation

pub mod layout;

#[cfg(all(test, feature = "plotnik-langs"))]
mod codegen_tests;
#[cfg(test)]
mod layout_tests;

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};

use crate::bytecode::ir::Label;
use crate::bytecode::{
    Entrypoint, FieldSymbol, Header, NodeSymbol, QTypeId, SECTION_ALIGN, StepId, StringId,
    TriviaEntry, TypeDef, TypeMember, TypeMetaHeader, TypeName,
};
use crate::type_system::TypeKind;

use crate::compile::Compiler;
use crate::query::query::LinkedQuery;
use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{
    FieldInfo, TYPE_NODE, TYPE_STRING, TYPE_VOID, TypeContext, TypeId, TypeShape,
};
use layout::CacheAligned;

/// Error during bytecode emission.
#[derive(Clone, Debug)]
pub enum EmitError {
    /// Query has validation errors (must be valid before emitting).
    InvalidQuery,
    /// Too many strings (exceeds u16 max).
    TooManyStrings(usize),
    /// Too many types (exceeds u16 max).
    TooManyTypes(usize),
    /// Too many type members (exceeds u16 max).
    TooManyTypeMembers(usize),
    /// Too many entrypoints (exceeds u16 max).
    TooManyEntrypoints(usize),
    /// Too many transitions (exceeds u16 max).
    TooManyTransitions(usize),
    /// String not found in interner.
    StringNotFound(Symbol),
    /// Compilation error.
    Compile(crate::compile::CompileError),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidQuery => write!(f, "query has validation errors"),
            Self::TooManyStrings(n) => write!(f, "too many strings: {n} (max 65534)"),
            Self::TooManyTypes(n) => write!(f, "too many types: {n} (max 65533)"),
            Self::TooManyTypeMembers(n) => write!(f, "too many type members: {n} (max 65535)"),
            Self::TooManyEntrypoints(n) => write!(f, "too many entrypoints: {n} (max 65535)"),
            Self::TooManyTransitions(n) => write!(f, "too many transitions: {n} (max 65535)"),
            Self::StringNotFound(sym) => write!(f, "string not found for symbol: {sym:?}"),
            Self::Compile(e) => write!(f, "compilation error: {e}"),
        }
    }
}

impl std::error::Error for EmitError {}

/// Easter egg string at index 0 (Dostoevsky, The Idiot).
/// StringId(0) is reserved and never referenced by instructions.
pub const EASTER_EGG: &str = "Beauty will save the world";

/// Builds the string table, remapping query Symbols to bytecode StringIds.
///
/// The bytecode format requires a subset of the query interner's strings.
/// This builder collects only the strings that are actually used and assigns
/// compact StringId indices.
///
/// StringId(0) is reserved for an easter egg and is never referenced by
/// instructions. Actual strings start at index 1.
#[derive(Debug)]
pub struct StringTableBuilder {
    /// Map from query Symbol to bytecode StringId.
    mapping: HashMap<Symbol, StringId>,
    /// Reverse lookup from string content to StringId (for intern_str).
    str_lookup: HashMap<String, StringId>,
    /// Ordered strings for the binary.
    strings: Vec<String>,
}

impl StringTableBuilder {
    pub fn new() -> Self {
        let mut builder = Self {
            mapping: HashMap::new(),
            str_lookup: HashMap::new(),
            strings: Vec::new(),
        };
        // Reserve index 0 for easter egg
        builder.strings.push(EASTER_EGG.to_string());
        builder.str_lookup.insert(EASTER_EGG.to_string(), StringId(0));
        builder
    }

    /// Get or create a StringId for a Symbol.
    pub fn get_or_intern(
        &mut self,
        sym: Symbol,
        interner: &Interner,
    ) -> Result<StringId, EmitError> {
        if let Some(&id) = self.mapping.get(&sym) {
            return Ok(id);
        }

        let text = interner
            .try_resolve(sym)
            .ok_or(EmitError::StringNotFound(sym))?;

        let id = StringId(self.strings.len() as u16);
        self.strings.push(text.to_string());
        self.str_lookup.insert(text.to_string(), id);
        self.mapping.insert(sym, id);
        Ok(id)
    }

    /// Intern a string directly (for generated strings not in the query interner).
    pub fn intern_str(&mut self, s: &str) -> StringId {
        if let Some(&id) = self.str_lookup.get(s) {
            return id;
        }

        let id = StringId(self.strings.len() as u16);
        self.strings.push(s.to_string());
        self.str_lookup.insert(s.to_string(), id);
        id
    }

    /// Number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the builder is empty.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Validate that the string count fits in u16.
    pub fn validate(&self) -> Result<(), EmitError> {
        // Max count is 65534 because the table needs count+1 entries.
        // Index 0 is reserved for the easter egg, so we can have 65533 user strings.
        if self.strings.len() > 65534 {
            return Err(EmitError::TooManyStrings(self.strings.len()));
        }
        Ok(())
    }

    /// Emit the string blob and offset table.
    ///
    /// Returns (blob_bytes, table_bytes).
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>) {
        let mut blob = Vec::new();
        let mut offsets: Vec<u32> = Vec::with_capacity(self.strings.len() + 1);

        for s in &self.strings {
            offsets.push(blob.len() as u32);
            blob.extend_from_slice(s.as_bytes());
        }
        offsets.push(blob.len() as u32); // sentinel

        // Convert offsets to bytes
        let table_bytes: Vec<u8> = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();

        (blob, table_bytes)
    }
}

impl Default for StringTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds the type metadata, remapping query TypeIds to bytecode QTypeIds.
#[derive(Debug)]
pub struct TypeTableBuilder {
    /// Map from query TypeId to bytecode QTypeId.
    mapping: HashMap<TypeId, QTypeId>,
    /// Type definitions (4 bytes each).
    type_defs: Vec<TypeDef>,
    /// Type members for structs/enums (4 bytes each).
    type_members: Vec<TypeMember>,
    /// Type names for named types (4 bytes each).
    type_names: Vec<TypeName>,
    /// Cache for dynamically created Optional wrappers: base_type -> Optional(base_type)
    optional_wrappers: HashMap<QTypeId, QTypeId>,
}

impl TypeTableBuilder {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
            type_defs: Vec::new(),
            type_members: Vec::new(),
            type_names: Vec::new(),
            optional_wrappers: HashMap::new(),
        }
    }

    /// Build type table from TypeContext.
    ///
    /// Types are collected in definition order, depth-first, to mirror query structure.
    /// Used builtins are emitted first, then custom types - no reserved slots.
    pub fn build(
        &mut self,
        type_ctx: &TypeContext,
        interner: &Interner,
        strings: &mut StringTableBuilder,
    ) -> Result<(), EmitError> {
        // Collect custom types in definition order, depth-first
        let mut ordered_types: Vec<TypeId> = Vec::new();
        let mut seen: HashSet<TypeId> = HashSet::new();

        for (_def_id, type_id) in type_ctx.iter_def_types() {
            collect_types_dfs(type_id, type_ctx, &mut ordered_types, &mut seen);
        }

        // Determine which builtins are actually used by scanning all types
        let mut used_builtins = [false; 3]; // [Void, Node, String]
        let mut seen = HashSet::new();
        for &type_id in &ordered_types {
            collect_builtin_refs(type_id, type_ctx, &mut used_builtins, &mut seen);
        }
        // Also check entrypoint result types directly
        for (_def_id, type_id) in type_ctx.iter_def_types() {
            if type_id == TYPE_VOID {
                used_builtins[0] = true;
            } else if type_id == TYPE_NODE {
                used_builtins[1] = true;
            } else if type_id == TYPE_STRING {
                used_builtins[2] = true;
            }
        }

        // Phase 1: Emit used builtins first (in order: Void, Node, String)
        let builtin_types = [(TYPE_VOID, TypeKind::Void), (TYPE_NODE, TypeKind::Node), (TYPE_STRING, TypeKind::String)];
        for (i, &(builtin_id, kind)) in builtin_types.iter().enumerate() {
            if used_builtins[i] {
                let bc_id = QTypeId(self.type_defs.len() as u16);
                self.mapping.insert(builtin_id, bc_id);
                self.type_defs.push(TypeDef {
                    data: 0,
                    count: 0,
                    kind: kind as u8,
                });
            }
        }

        // Phase 2: Pre-assign QTypeIds for custom types and reserve slots
        for &type_id in &ordered_types {
            let bc_id = QTypeId(self.type_defs.len() as u16);
            self.mapping.insert(type_id, bc_id);
            self.type_defs.push(TypeDef {
                data: 0,
                count: 0,
                kind: 0, // Placeholder
            });
        }

        // Phase 3: Fill in custom type definitions
        // We need to calculate slot index as offset from where custom types start
        let builtin_count = used_builtins.iter().filter(|&&b| b).count();
        for (i, &type_id) in ordered_types.iter().enumerate() {
            let slot_index = builtin_count + i;
            let type_shape = type_ctx
                .get_type(type_id)
                .expect("collected type must exist");
            self.emit_type_at_slot(slot_index, type_id, type_shape, type_ctx, interner, strings)?;
        }

        // Collect TypeName entries for named definitions
        for (def_id, type_id) in type_ctx.iter_def_types() {
            let name_sym = type_ctx.def_name_sym(def_id);
            let name = strings.get_or_intern(name_sym, interner)?;
            let bc_type_id = self.mapping.get(&type_id).copied().unwrap_or(QTypeId(0));
            self.type_names.push(TypeName {
                name,
                type_id: bc_type_id,
            });
        }

        Ok(())
    }

    /// Fill in a TypeDef at a pre-allocated slot.
    fn emit_type_at_slot(
        &mut self,
        slot_index: usize,
        _type_id: TypeId,
        type_shape: &TypeShape,
        type_ctx: &TypeContext,
        interner: &Interner,
        strings: &mut StringTableBuilder,
    ) -> Result<(), EmitError> {
        match type_shape {
            TypeShape::Void | TypeShape::Node | TypeShape::String => {
                // Builtins - should not reach here
                unreachable!("builtins should be handled separately")
            }

            TypeShape::Custom(sym) => {
                // Custom type annotation: @x :: Identifier → type Identifier = Node
                let bc_type_id = QTypeId(slot_index as u16);

                // Add TypeName entry for the custom type
                let name = strings.get_or_intern(*sym, interner)?;
                self.type_names.push(TypeName {
                    name,
                    type_id: bc_type_id,
                });

                // Custom types alias Node - look up Node's actual bytecode ID
                let node_bc_id = self.mapping.get(&TYPE_NODE).copied().unwrap_or(QTypeId(0));
                self.type_defs[slot_index] = TypeDef {
                    data: node_bc_id.0,
                    count: 0,
                    kind: TypeKind::Alias as u8,
                };
                Ok(())
            }

            TypeShape::Optional(inner) => {
                let inner_bc = self.resolve_type(*inner, type_ctx)?;

                self.type_defs[slot_index] = TypeDef {
                    data: inner_bc.0,
                    count: 0,
                    kind: TypeKind::Optional as u8,
                };
                Ok(())
            }

            TypeShape::Array { element, non_empty } => {
                let element_bc = self.resolve_type(*element, type_ctx)?;

                let kind = if *non_empty {
                    TypeKind::ArrayOneOrMore
                } else {
                    TypeKind::ArrayZeroOrMore
                };
                self.type_defs[slot_index] = TypeDef {
                    data: element_bc.0,
                    count: 0,
                    kind: kind as u8,
                };
                Ok(())
            }

            TypeShape::Struct(fields) => {
                // Resolve field types (this may create Optional wrappers at later indices)
                let mut resolved_fields = Vec::with_capacity(fields.len());
                for (field_sym, field_info) in fields {
                    let field_name = strings.get_or_intern(*field_sym, interner)?;
                    let field_type = self.resolve_field_type(field_info, type_ctx)?;
                    resolved_fields.push((field_name, field_type));
                }

                // Now emit the members and update the placeholder
                let member_start = self.type_members.len() as u16;
                for (field_name, field_type) in resolved_fields {
                    self.type_members.push(TypeMember {
                        name: field_name,
                        type_id: field_type,
                    });
                }

                let member_count = fields.len() as u8;
                self.type_defs[slot_index] = TypeDef {
                    data: member_start,
                    count: member_count,
                    kind: TypeKind::Struct as u8,
                };
                Ok(())
            }

            TypeShape::Enum(variants) => {
                // Resolve variant types (this may create types at later indices)
                let mut resolved_variants = Vec::with_capacity(variants.len());
                for (variant_sym, variant_type_id) in variants {
                    let variant_name = strings.get_or_intern(*variant_sym, interner)?;
                    let variant_type = self.resolve_type(*variant_type_id, type_ctx)?;
                    resolved_variants.push((variant_name, variant_type));
                }

                // Now emit the members and update the placeholder
                let member_start = self.type_members.len() as u16;
                for (variant_name, variant_type) in resolved_variants {
                    self.type_members.push(TypeMember {
                        name: variant_name,
                        type_id: variant_type,
                    });
                }

                let member_count = variants.len() as u8;
                self.type_defs[slot_index] = TypeDef {
                    data: member_start,
                    count: member_count,
                    kind: TypeKind::Enum as u8,
                };
                Ok(())
            }

            TypeShape::Ref(_def_id) => {
                // Ref types are not emitted - they resolve to their target
                unreachable!("Ref types should not be collected for emission")
            }
        }
    }

    /// Resolve a query TypeId to bytecode QTypeId.
    fn resolve_type(&self, type_id: TypeId, type_ctx: &TypeContext) -> Result<QTypeId, EmitError> {
        // Check if already mapped
        if let Some(&bc_id) = self.mapping.get(&type_id) {
            return Ok(bc_id);
        }

        // Handle Ref types by following the reference
        if let Some(type_shape) = type_ctx.get_type(type_id)
            && let TypeShape::Ref(def_id) = type_shape
            && let Some(def_type_id) = type_ctx.get_def_type(*def_id)
        {
            return self.resolve_type(def_type_id, type_ctx);
        }

        // If not found, default to first type (should not happen for well-formed types)
        Ok(QTypeId(0))
    }

    /// Resolve a field's type, handling optionality.
    fn resolve_field_type(
        &mut self,
        field_info: &FieldInfo,
        type_ctx: &TypeContext,
    ) -> Result<QTypeId, EmitError> {
        let base_type = self.resolve_type(field_info.type_id, type_ctx)?;

        // If the field is optional, wrap it in Optional
        if field_info.optional {
            self.get_or_create_optional(base_type)
        } else {
            Ok(base_type)
        }
    }

    /// Get or create an Optional wrapper for a base type.
    fn get_or_create_optional(&mut self, base_type: QTypeId) -> Result<QTypeId, EmitError> {
        // Check cache first
        if let Some(&optional_id) = self.optional_wrappers.get(&base_type) {
            return Ok(optional_id);
        }

        // Create new Optional wrapper at the next available index
        let optional_id = QTypeId(self.type_defs.len() as u16);

        self.type_defs.push(TypeDef {
            data: base_type.0,
            count: 0,
            kind: TypeKind::Optional as u8,
        });

        self.optional_wrappers.insert(base_type, optional_id);
        Ok(optional_id)
    }

    /// Validate that counts fit in u16.
    pub fn validate(&self) -> Result<(), EmitError> {
        if self.type_defs.len() > 65535 {
            return Err(EmitError::TooManyTypes(self.type_defs.len()));
        }
        if self.type_members.len() > 65535 {
            return Err(EmitError::TooManyTypeMembers(self.type_members.len()));
        }
        Ok(())
    }

    /// Get the bytecode QTypeId for a query TypeId.
    pub fn get(&self, type_id: TypeId) -> Option<QTypeId> {
        self.mapping.get(&type_id).copied()
    }

    /// Get the absolute member base index for a struct/enum type.
    ///
    /// For Struct and Enum types, returns the starting index in the TypeMembers table.
    /// Fields/variants are at indices [base..base+count).
    pub fn get_member_base(&self, type_id: TypeId) -> Option<u16> {
        let bc_type_id = self.mapping.get(&type_id)?;
        let type_def = self.type_defs.get(bc_type_id.0 as usize)?;

        // Only Struct and Enum have member bases
        let kind = TypeKind::from_u8(type_def.kind)?;
        if kind.is_composite() {
            Some(type_def.data)
        } else {
            None
        }
    }

    /// Emit type definitions, members, and names as bytes.
    ///
    /// Returns (type_defs_bytes, type_members_bytes, type_names_bytes).
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut defs_bytes = Vec::with_capacity(self.type_defs.len() * 4);
        for def in &self.type_defs {
            defs_bytes.extend_from_slice(&def.data.to_le_bytes());
            defs_bytes.push(def.count);
            defs_bytes.push(def.kind);
        }

        let mut members_bytes = Vec::with_capacity(self.type_members.len() * 4);
        for member in &self.type_members {
            members_bytes.extend_from_slice(&member.name.0.to_le_bytes());
            members_bytes.extend_from_slice(&member.type_id.0.to_le_bytes());
        }

        let mut names_bytes = Vec::with_capacity(self.type_names.len() * 4);
        for type_name in &self.type_names {
            names_bytes.extend_from_slice(&type_name.name.0.to_le_bytes());
            names_bytes.extend_from_slice(&type_name.type_id.0.to_le_bytes());
        }

        (defs_bytes, members_bytes, names_bytes)
    }

    /// Number of type definitions.
    pub fn type_defs_count(&self) -> usize {
        self.type_defs.len()
    }

    /// Number of type members.
    pub fn type_members_count(&self) -> usize {
        self.type_members.len()
    }

    /// Number of type names.
    pub fn type_names_count(&self) -> usize {
        self.type_names.len()
    }
}

impl Default for TypeTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect types depth-first starting from a root type.
fn collect_types_dfs(
    type_id: TypeId,
    type_ctx: &TypeContext,
    out: &mut Vec<TypeId>,
    seen: &mut HashSet<TypeId>,
) {
    // Skip builtins and already-seen types
    if type_id.is_builtin() || seen.contains(&type_id) {
        return;
    }

    let Some(type_shape) = type_ctx.get_type(type_id) else {
        return;
    };

    // Resolve Ref types to their target
    if let TypeShape::Ref(def_id) = type_shape {
        if let Some(target_id) = type_ctx.get_def_type(*def_id) {
            collect_types_dfs(target_id, type_ctx, out, seen);
        }
        return;
    }

    seen.insert(type_id);

    // Collect children first (depth-first), then add self
    match type_shape {
        TypeShape::Struct(fields) => {
            for field_info in fields.values() {
                collect_types_dfs(field_info.type_id, type_ctx, out, seen);
            }
            out.push(type_id);
        }
        TypeShape::Enum(variants) => {
            for &variant_type_id in variants.values() {
                collect_types_dfs(variant_type_id, type_ctx, out, seen);
            }
            out.push(type_id);
        }
        TypeShape::Array { element, .. } => {
            // Collect element type first, then add the Array itself
            collect_types_dfs(*element, type_ctx, out, seen);
            out.push(type_id);
        }
        TypeShape::Optional(inner) => {
            // Collect inner type first, then add the Optional itself
            collect_types_dfs(*inner, type_ctx, out, seen);
            out.push(type_id);
        }
        TypeShape::Custom(_) => {
            // Custom types alias Node, no children to collect
            out.push(type_id);
        }
        _ => {}
    }
}

/// Collect which builtin types are referenced by a type.
fn collect_builtin_refs(
    type_id: TypeId,
    type_ctx: &TypeContext,
    used: &mut [bool; 3],
    seen: &mut HashSet<TypeId>,
) {
    if !seen.insert(type_id) {
        return;
    }

    let Some(type_shape) = type_ctx.get_type(type_id) else {
        return;
    };

    match type_shape {
        TypeShape::Void => used[0] = true,
        TypeShape::Node => used[1] = true,
        TypeShape::String => used[2] = true,
        TypeShape::Custom(_) => used[1] = true, // Custom types alias Node
        TypeShape::Struct(fields) => {
            for field_info in fields.values() {
                collect_builtin_refs(field_info.type_id, type_ctx, used, seen);
            }
        }
        TypeShape::Enum(variants) => {
            for &variant_type_id in variants.values() {
                collect_builtin_refs(variant_type_id, type_ctx, used, seen);
            }
        }
        TypeShape::Array { element, .. } => {
            collect_builtin_refs(*element, type_ctx, used, seen);
        }
        TypeShape::Optional(inner) => {
            collect_builtin_refs(*inner, type_ctx, used, seen);
        }
        TypeShape::Ref(def_id) => {
            if let Some(target_id) = type_ctx.get_def_type(*def_id) {
                collect_builtin_refs(target_id, type_ctx, used, seen);
            }
        }
    }
}

/// Pad a buffer to the section alignment boundary.
fn pad_to_section(buf: &mut Vec<u8>) {
    let rem = buf.len() % SECTION_ALIGN;
    if rem != 0 {
        let padding = SECTION_ALIGN - rem;
        buf.resize(buf.len() + padding, 0);
    }
}

/// Emit bytecode from type context only (no node validation).
pub fn emit(
    type_ctx: &TypeContext,
    interner: &Interner,
    symbol_table: &SymbolTable,
) -> Result<Vec<u8>, EmitError> {
    emit_inner(type_ctx, interner, symbol_table, None, None)
}

/// Emit bytecode from a LinkedQuery (includes node type/field validation info).
pub fn emit_linked(query: &LinkedQuery) -> Result<Vec<u8>, EmitError> {
    emit_inner(
        query.type_context(),
        query.interner(),
        &query.symbol_table,
        Some(query.node_type_ids()),
        Some(query.node_field_ids()),
    )
}

/// Shared bytecode emission logic.
fn emit_inner(
    type_ctx: &TypeContext,
    interner: &Interner,
    symbol_table: &SymbolTable,
    node_type_ids: Option<&IndexMap<Symbol, NodeTypeId>>,
    node_field_ids: Option<&IndexMap<Symbol, NodeFieldId>>,
) -> Result<Vec<u8>, EmitError> {
    let is_linked = node_type_ids.is_some();
    let mut strings = StringTableBuilder::new();
    let mut types = TypeTableBuilder::new();
    types.build(type_ctx, interner, &mut strings)?;

    // Compile transitions (strings are interned here for unlinked mode)
    let compile_result = Compiler::compile(interner, type_ctx, symbol_table, &mut strings, node_type_ids, node_field_ids)
        .map_err(EmitError::Compile)?;

    // Layout with cache alignment
    let entry_labels: Vec<Label> = compile_result.def_entries.values().copied().collect();
    let layout = CacheAligned::layout(&compile_result.instructions, &entry_labels);

    // Validate transition count
    if layout.total_steps as usize > 65535 {
        return Err(EmitError::TooManyTransitions(layout.total_steps as usize));
    }

    // Collect node symbols (empty if not linked)
    let mut node_symbols: Vec<NodeSymbol> = Vec::new();
    if let Some(ids) = node_type_ids {
        for (&sym, &node_id) in ids {
            let name = strings.get_or_intern(sym, interner)?;
            node_symbols.push(NodeSymbol {
                id: node_id.get(),
                name,
            });
        }
    }

    // Collect field symbols (empty if not linked)
    let mut field_symbols: Vec<FieldSymbol> = Vec::new();
    if let Some(ids) = node_field_ids {
        for (&sym, &field_id) in ids {
            let name = strings.get_or_intern(sym, interner)?;
            field_symbols.push(FieldSymbol {
                id: field_id.get(),
                name,
            });
        }
    }

    // Collect entrypoints with actual targets from layout
    let mut entrypoints: Vec<Entrypoint> = Vec::new();
    for (def_id, type_id) in type_ctx.iter_def_types() {
        let name_sym = type_ctx.def_name_sym(def_id);
        let name = strings.get_or_intern(name_sym, interner)?;
        let result_type = types.get(type_id).expect("all def types should be mapped");

        // Get actual target from compiled result
        let target = compile_result
            .def_entries
            .get(&def_id)
            .and_then(|label| layout.label_to_step.get(label))
            .copied()
            .unwrap_or(StepId::ACCEPT);

        entrypoints.push(Entrypoint {
            name,
            target,
            result_type,
            _pad: 0,
        });
    }

    // Validate counts
    strings.validate()?;
    types.validate()?;
    if entrypoints.len() > 65535 {
        return Err(EmitError::TooManyEntrypoints(entrypoints.len()));
    }

    // Trivia (empty for now)
    let trivia_entries: Vec<TriviaEntry> = Vec::new();

    // Resolve and serialize transitions
    let transitions_bytes = emit_transitions(&compile_result.instructions, &layout, &types);

    // Emit all byte sections
    let (str_blob, str_table) = strings.emit();
    let (type_defs_bytes, type_members_bytes, type_names_bytes) = types.emit();

    let node_types_bytes = emit_node_symbols(&node_symbols);
    let node_fields_bytes = emit_field_symbols(&field_symbols);
    let trivia_bytes = emit_trivia(&trivia_entries);
    let entrypoints_bytes = emit_entrypoints(&entrypoints);

    // Build output with sections
    let mut output = vec![0u8; 64]; // Reserve header space

    let str_blob_offset = emit_section(&mut output, &str_blob);
    let str_table_offset = emit_section(&mut output, &str_table);
    let node_types_offset = emit_section(&mut output, &node_types_bytes);
    let node_fields_offset = emit_section(&mut output, &node_fields_bytes);
    let trivia_offset = emit_section(&mut output, &trivia_bytes);

    // Type metadata section (header + 3 aligned sub-sections)
    let type_meta_offset = emit_section(
        &mut output,
        &TypeMetaHeader {
            type_defs_count: types.type_defs_count() as u16,
            type_members_count: types.type_members_count() as u16,
            type_names_count: types.type_names_count() as u16,
            _pad: 0,
        }
        .to_bytes(),
    );
    emit_section(&mut output, &type_defs_bytes);
    emit_section(&mut output, &type_members_bytes);
    emit_section(&mut output, &type_names_bytes);

    let entrypoints_offset = emit_section(&mut output, &entrypoints_bytes);
    let transitions_offset = emit_section(&mut output, &transitions_bytes);

    pad_to_section(&mut output);
    let total_size = output.len() as u32;

    // Build and write header
    let mut header = Header {
        str_blob_offset,
        str_table_offset,
        node_types_offset,
        node_fields_offset,
        trivia_offset,
        type_meta_offset,
        entrypoints_offset,
        transitions_offset,
        str_table_count: strings.len() as u16,
        node_types_count: node_symbols.len() as u16,
        node_fields_count: field_symbols.len() as u16,
        trivia_count: trivia_entries.len() as u16,
        entrypoints_count: entrypoints.len() as u16,
        transitions_count: layout.total_steps,
        total_size,
        ..Default::default()
    };
    header.set_linked(is_linked);
    header.checksum = crc32fast::hash(&output[64..]);
    output[..64].copy_from_slice(&header.to_bytes());

    Ok(output)
}

/// Emit transitions section from instructions and layout.
fn emit_transitions(
    instructions: &[crate::bytecode::ir::Instruction],
    layout: &crate::bytecode::ir::LayoutResult,
    types: &TypeTableBuilder,
) -> Vec<u8> {
    // Allocate buffer for all steps (8 bytes each)
    let mut bytes = vec![0u8; layout.total_steps as usize * 8];

    // Create a resolver closure for member indices
    let get_member_base = |type_id: TypeId| types.get_member_base(type_id);

    for instr in instructions {
        let label = instr.label();
        let Some(&step_id) = layout.label_to_step.get(&label) else {
            continue;
        };

        let offset = step_id.byte_offset();
        let resolved = instr.resolve(&layout.label_to_step, get_member_base);

        // Copy instruction bytes to the correct position
        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    bytes
}

fn emit_section(output: &mut Vec<u8>, data: &[u8]) -> u32 {
    pad_to_section(output);
    let offset = output.len() as u32;
    output.extend_from_slice(data);
    offset
}

fn emit_node_symbols(symbols: &[NodeSymbol]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * 4);
    for sym in symbols {
        bytes.extend_from_slice(&sym.id.to_le_bytes());
        bytes.extend_from_slice(&sym.name.0.to_le_bytes());
    }
    bytes
}

fn emit_field_symbols(symbols: &[FieldSymbol]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * 4);
    for sym in symbols {
        bytes.extend_from_slice(&sym.id.to_le_bytes());
        bytes.extend_from_slice(&sym.name.0.to_le_bytes());
    }
    bytes
}

fn emit_trivia(entries: &[TriviaEntry]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        bytes.extend_from_slice(&entry.node_type.to_le_bytes());
    }
    bytes
}

fn emit_entrypoints(entrypoints: &[Entrypoint]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entrypoints.len() * 8);
    for ep in entrypoints {
        bytes.extend_from_slice(&ep.name.0.to_le_bytes());
        bytes.extend_from_slice(&ep.target.0.to_le_bytes());
        bytes.extend_from_slice(&ep.result_type.0.to_le_bytes());
        bytes.extend_from_slice(&ep._pad.to_le_bytes());
    }
    bytes
}
