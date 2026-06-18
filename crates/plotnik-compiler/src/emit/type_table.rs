//! Type table builder for bytecode emission.
//!
//! Converts query-level types (TypeContext) into bytecode-level types (BytecodeTypeId).

use std::collections::{HashMap, HashSet};

use plotnik_core::Interner;

use crate::analyze::type_check::{FieldInfo, TYPE_NODE, TYPE_VOID, TypeContext, TypeId, TypeShape};
use plotnik_bytecode::{
    TypeData, TypeDef, TypeId as BytecodeTypeId, TypeKind, TypeMember, TypeName,
};

use super::{EmitError, StringTableBuilder};

/// Builds the type metadata, remapping query TypeIds to bytecode BytecodeTypeIds.
#[derive(Debug)]
pub struct TypeTableBuilder {
    /// Map from query TypeId to bytecode BytecodeTypeId.
    mapping: HashMap<TypeId, BytecodeTypeId>,
    /// Type definitions (4 bytes each).
    type_defs: Vec<TypeDef>,
    /// Type members for structs/enums (4 bytes each).
    type_members: Vec<TypeMember>,
    /// Type names for named types (4 bytes each).
    type_names: Vec<TypeName>,
    /// Cache for dynamically created Optional wrappers: base_type -> Optional(base_type)
    optional_wrappers: HashMap<BytecodeTypeId, BytecodeTypeId>,
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

    /// Build the type table, remapping query TypeIds to bytecode ids.
    ///
    /// Only types reachable from an entrypoint result are emitted. Dead
    /// intermediate types produced during inference — an untagged alternation's
    /// per-branch merge structs, for instance — are pruned. Used builtins are
    /// emitted first, then custom types in definition order, depth-first.
    pub fn build(
        &mut self,
        type_ctx: &TypeContext,
        interner: &Interner,
        strings: &mut StringTableBuilder,
    ) -> Result<(), EmitError> {
        // Collect custom types depth-first from definition result types. Every
        // emitted effect's member ref names a type that one of these reaches, so
        // this single walk also covers all effect-referenced types. Definition
        // order fixes the emission order entrypoints rely on.
        let mut ordered_types: Vec<TypeId> = Vec::new();
        let mut seen: HashSet<TypeId> = HashSet::new();

        for (_def_id, type_id) in type_ctx.iter_def_types() {
            collect_types_dfs(type_id, type_ctx, &mut ordered_types, &mut seen);

            if !matches!(type_ctx.get_type(type_id), Some(TypeShape::Ref(_))) {
                continue;
            }

            if !seen.insert(type_id) {
                continue;
            }

            ordered_types.push(type_id);
        }

        // Determine which builtins are actually used by scanning all types
        let mut used_builtins = [false; 2]; // [Void, Node]
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
            }
        }

        // Phase 1: Emit used builtins first (in order: Void, Node)
        let builtin_types = [(TYPE_VOID, TypeKind::Void), (TYPE_NODE, TypeKind::Node)];
        for (i, &(builtin_id, kind)) in builtin_types.iter().enumerate() {
            if used_builtins[i] {
                let bc_id = BytecodeTypeId(self.type_defs.len() as u16);
                self.mapping.insert(builtin_id, bc_id);
                self.type_defs.push(TypeDef::builtin(kind));
            }
        }

        // Phase 2: Pre-assign BytecodeTypeIds for custom types and reserve slots
        for &type_id in &ordered_types {
            let bc_id = BytecodeTypeId(self.type_defs.len() as u16);
            self.mapping.insert(type_id, bc_id);
            self.type_defs.push(TypeDef::placeholder());
        }

        // Phase 3: Fill in custom type definitions
        // We need to calculate slot index as offset from where custom types start
        let builtin_count = used_builtins.iter().filter(|&&b| b).count();
        for (i, &type_id) in ordered_types.iter().enumerate() {
            let slot_index = builtin_count + i;
            let type_shape = type_ctx
                .get_type(type_id)
                .expect("collected type must exist");
            self.emit_type_at_slot(slot_index, type_shape, type_ctx, interner, strings)?;
        }

        for (def_id, type_id) in type_ctx.iter_def_types() {
            let name_sym = type_ctx.def_name_sym(def_id);
            let name = strings.get_or_intern(name_sym, interner)?;
            let bc_type_id = self
                .mapping
                .get(&type_id)
                .copied()
                .expect("def result type must be mapped");
            self.type_names.push(TypeName::new(name, bc_type_id));
        }

        // Collect TypeName entries for explicit type annotations on struct captures,
        // e.g. `{(fn) @fn} @outer :: FunctionInfo` names the struct "FunctionInfo".
        // A name only attaches to a non-suppressive capture's struct/enum, so the
        // type is reachable from a def result and must survive dead-type elimination;
        // a miss here is a compiler bug, not anything a query can trigger.
        for (type_id, name_sym) in type_ctx.iter_type_names() {
            let bc_type_id = self
                .mapping
                .get(&type_id)
                .copied()
                .expect("named type annotation must survive dead-type elimination");
            let name = strings.get_or_intern(name_sym, interner)?;
            self.type_names.push(TypeName::new(name, bc_type_id));
        }

        Ok(())
    }

    fn emit_type_at_slot(
        &mut self,
        slot_index: usize,
        type_shape: &TypeShape,
        type_ctx: &TypeContext,
        interner: &Interner,
        strings: &mut StringTableBuilder,
    ) -> Result<(), EmitError> {
        match type_shape {
            TypeShape::Void | TypeShape::Node => {
                unreachable!("builtins should be handled separately")
            }

            TypeShape::Custom(sym) => {
                // Custom type annotation: @x :: Identifier → type Identifier = Node
                let bc_type_id = BytecodeTypeId(slot_index as u16);

                let name = strings.get_or_intern(*sym, interner)?;
                self.type_names.push(TypeName::new(name, bc_type_id));

                // Custom types alias Node - look up Node's actual bytecode ID.
                // Reaching a Custom type means it was in `ordered_types`, so
                // `collect_builtin_refs` marked Node used (type_table.rs: `Custom(_) =>
                // used[1] = true`) and Phase 1 emitted it into `mapping`.
                let node_bc_id =
                    self.mapping.get(&TYPE_NODE).copied().expect(
                        "Node must be mapped before a Custom alias that targets it is emitted",
                    );
                self.type_defs[slot_index] = TypeDef::alias(node_bc_id);
                Ok(())
            }

            TypeShape::Optional(inner) => {
                let inner_bc = self.resolve_type(*inner, type_ctx)?;
                self.type_defs[slot_index] = TypeDef::optional(inner_bc);
                Ok(())
            }

            TypeShape::Array { element, non_empty } => {
                let element_bc = self.resolve_type(*element, type_ctx)?;
                self.type_defs[slot_index] = if *non_empty {
                    TypeDef::array_plus(element_bc)
                } else {
                    TypeDef::array_star(element_bc)
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

                let member_start = self.type_members.len() as u16;
                for (field_name, field_type) in resolved_fields {
                    self.type_members
                        .push(TypeMember::new(field_name, field_type));
                }

                let member_count = u8::try_from(fields.len())
                    .map_err(|_| EmitError::TooManyFields(fields.len()))?;
                self.type_defs[slot_index] = TypeDef::struct_type(member_start, member_count);
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

                let member_start = self.type_members.len() as u16;
                for (variant_name, variant_type) in resolved_variants {
                    self.type_members
                        .push(TypeMember::new(variant_name, variant_type));
                }

                let member_count = u8::try_from(variants.len())
                    .map_err(|_| EmitError::TooManyVariants(variants.len()))?;
                self.type_defs[slot_index] = TypeDef::enum_type(member_start, member_count);
                Ok(())
            }

            TypeShape::Ref(def_id) => {
                let target = type_ctx
                    .get_def_type(*def_id)
                    .expect("alias def target must exist");
                self.type_defs[slot_index] = TypeDef::alias(self.resolve_type(target, type_ctx)?);
                Ok(())
            }
        }
    }

    /// Resolve a query TypeId to its underlying bytecode BytecodeTypeId.
    ///
    /// Ref types are emitted as aliases only when they are definition results. In
    /// every materialized position, follow the reference chain to the actual shape.
    pub fn resolve_type(
        &self,
        type_id: TypeId,
        type_ctx: &TypeContext,
    ) -> Result<BytecodeTypeId, EmitError> {
        let type_id = self.resolve_underlying_type_id(type_id, type_ctx);
        let bc_id = self
            .mapping
            .get(&type_id)
            .copied()
            .expect("resolved type must be mapped");
        Ok(bc_id)
    }

    fn resolve_underlying_type_id(&self, type_id: TypeId, type_ctx: &TypeContext) -> TypeId {
        let Some(TypeShape::Ref(def_id)) = type_ctx.get_type(type_id) else {
            return type_id;
        };

        let target = type_ctx
            .get_def_type(*def_id)
            .expect("ref target def type must exist");
        self.resolve_underlying_type_id(target, type_ctx)
    }

    fn resolve_field_type(
        &mut self,
        field_info: &FieldInfo,
        type_ctx: &TypeContext,
    ) -> Result<BytecodeTypeId, EmitError> {
        let base_type = self.resolve_type(field_info.type_id, type_ctx)?;

        if field_info.optional {
            self.get_or_create_optional(base_type)
        } else {
            Ok(base_type)
        }
    }

    fn get_or_create_optional(
        &mut self,
        base_type: BytecodeTypeId,
    ) -> Result<BytecodeTypeId, EmitError> {
        if let Some(&optional_id) = self.optional_wrappers.get(&base_type) {
            return Ok(optional_id);
        }

        let optional_id = BytecodeTypeId(self.type_defs.len() as u16);
        self.type_defs.push(TypeDef::optional(base_type));
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

    pub fn get(&self, type_id: TypeId) -> Option<BytecodeTypeId> {
        self.mapping.get(&type_id).copied()
    }

    /// Get the absolute member base index for a struct/enum type.
    ///
    /// For Struct and Enum types, returns the starting index in the TypeMembers table.
    /// Fields/variants are at indices [base..base+count).
    pub fn get_member_base(&self, type_id: TypeId) -> Option<u16> {
        let bc_type_id = self.mapping.get(&type_id)?;
        let type_def = self.type_defs.get(bc_type_id.0 as usize)?;
        match type_def.classify() {
            TypeData::Composite { member_start, .. } => Some(member_start),
            _ => None,
        }
    }

    /// Returns `(type_defs_bytes, type_members_bytes, type_names_bytes)`.
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut defs_bytes = Vec::with_capacity(self.type_defs.len() * TypeDef::SIZE);
        for def in &self.type_defs {
            defs_bytes.extend_from_slice(&def.to_bytes());
        }

        let mut members_bytes = Vec::with_capacity(self.type_members.len() * TypeMember::SIZE);
        for member in &self.type_members {
            members_bytes.extend_from_slice(&member.to_bytes());
        }

        let mut names_bytes = Vec::with_capacity(self.type_names.len() * TypeName::SIZE);
        for type_name in &self.type_names {
            names_bytes.extend_from_slice(&type_name.to_bytes());
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

fn collect_types_dfs(
    type_id: TypeId,
    type_ctx: &TypeContext,
    out: &mut Vec<TypeId>,
    seen: &mut HashSet<TypeId>,
) {
    if type_id.is_builtin() || seen.contains(&type_id) {
        return;
    }

    let Some(type_shape) = type_ctx.get_type(type_id) else {
        return;
    };

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
            collect_types_dfs(*element, type_ctx, out, seen);
            out.push(type_id);
        }
        TypeShape::Optional(inner) => {
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

fn collect_builtin_refs(
    type_id: TypeId,
    type_ctx: &TypeContext,
    used: &mut [bool; 2],
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
