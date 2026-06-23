//! Type table builder for bytecode emission.
//!
//! Converts query-level types (TypeAnalysis) into bytecode-level types (BytecodeTypeId).

use std::collections::{HashMap, HashSet};

use plotnik_core::Interner;

use plotnik_bytecode::{
    TypeDefKind, TypeDef, TypeId as BytecodeTypeId, TypeKind, TypeMember, TypeNameEntry,
};

use crate::type_shape::{FieldInfo, TYPE_NODE, TYPE_VOID, TypeShape};
use crate::{DependencyAnalysis, TypeAnalysis, TypeId};

use super::error::EmitError;
use super::string_table::StringTableBuilder;

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
    type_names: Vec<TypeNameEntry>,
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
    /// intermediate types produced during inference — a union alternation's
    /// per-branch merge structs, for instance — are pruned. Used builtins are
    /// emitted first, then custom types in definition order, depth-first.
    pub fn build(
        &mut self,
        type_ctx: &TypeAnalysis,
        dependency_analysis: &DependencyAnalysis,
        interner: &Interner,
        strings: &mut StringTableBuilder,
    ) -> Result<(), EmitError> {
        // Collect custom types depth-first from definition result types. Every
        // emitted effect's member ref names a type that one of these reaches, so
        // this single walk also covers all effect-referenced types. Definition
        // order fixes the emission order entrypoints rely on.
        let mut collector = TypeCollector::new();
        for (_def_id, type_id) in type_ctx.iter_def_output() {
            collector.collect(type_id, type_ctx);

            if !matches!(type_ctx.type_shape(type_id), Some(TypeShape::Ref(_))) {
                continue;
            }

            if !collector.seen.insert(type_id) {
                continue;
            }

            collector.out.push(type_id);
        }
        let ordered_types = collector.out;

        // Determine which builtins are actually used by scanning all types
        let mut usage = BuiltinUses::new();
        for &type_id in &ordered_types {
            usage.collect(type_id, type_ctx);
        }
        // Also check entrypoint result types directly
        for (_def_id, type_id) in type_ctx.iter_def_output() {
            if type_id == TYPE_VOID {
                usage.uses_void = true;
            } else if type_id == TYPE_NODE {
                usage.uses_node = true;
            }
        }

        // Phase 1: Emit used builtins first (in order: Void, Node)
        let builtin_types = [
            (TYPE_VOID, TypeKind::Void, usage.uses_void),
            (TYPE_NODE, TypeKind::Node, usage.uses_node),
        ];
        for &(builtin_id, kind, used) in &builtin_types {
            if used {
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
        let builtin_count = usage.uses_void as usize + usage.uses_node as usize;
        let mut ctx = TypeEmitSlots {
            type_ctx,
            interner,
            strings,
        };
        for (i, &type_id) in ordered_types.iter().enumerate() {
            let slot_index = builtin_count + i;
            let type_shape = type_ctx
                .type_shape(type_id)
                .expect("collected type must exist");
            self.emit_type_at_slot(slot_index, type_shape, &mut ctx)?;
        }

        for (def_id, type_id) in type_ctx.iter_def_output() {
            let name_sym = dependency_analysis.def_name_sym(def_id);
            let name = ctx.strings.get_or_intern(name_sym, ctx.interner)?;
            let bc_type_id = self
                .mapping
                .get(&type_id)
                .copied()
                .expect("def result type must be mapped");
            self.type_names.push(TypeNameEntry::new(name, bc_type_id));
        }

        // Collect TypeNameEntry entries for explicit type annotations on struct captures,
        // e.g. `{(fn) @fn} @outer :: FunctionInfo` names the struct "FunctionInfo".
        // A name only attaches to a non-suppressive capture's struct/enum, so the
        // type is reachable from a def result and must survive dead-type elimination;
        // a miss here is a compiler bug, not anything a query can trigger.
        for (type_id, name_sym) in type_ctx.iter_type_aliases() {
            let bc_type_id = self
                .mapping
                .get(&type_id)
                .copied()
                .expect("named type annotation must survive dead-type elimination");
            let name = ctx.strings.get_or_intern(name_sym, ctx.interner)?;
            self.type_names.push(TypeNameEntry::new(name, bc_type_id));
        }

        Ok(())
    }

    fn emit_type_at_slot(
        &mut self,
        slot_index: usize,
        type_shape: &TypeShape,
        ctx: &mut TypeEmitSlots,
    ) -> Result<(), EmitError> {
        match type_shape {
            TypeShape::Void | TypeShape::Node => {
                unreachable!("builtins should be handled separately")
            }

            TypeShape::Custom(sym) => {
                // Custom type annotation: @x :: Identifier → type Identifier = Node
                let bc_type_id = BytecodeTypeId(slot_index as u16);

                let name = ctx.strings.get_or_intern(*sym, ctx.interner)?;
                self.type_names.push(TypeNameEntry::new(name, bc_type_id));

                // Custom types alias Node - look up Node's actual bytecode ID.
                // Reaching a Custom type means it was in `ordered_types`, so
                // `BuiltinUses::collect` marked Node used (type_table.rs: `Custom(_) =>
                // usage.node = true`) and Phase 1 emitted it into `mapping`.
                let node_bc_id =
                    self.mapping.get(&TYPE_NODE).copied().expect(
                        "Node must be mapped before a Custom alias that targets it is emitted",
                    );
                self.type_defs[slot_index] = TypeDef::alias(node_bc_id);
                Ok(())
            }

            TypeShape::Optional(inner) => {
                let inner_bc = self.resolve_type(*inner, ctx.type_ctx)?;
                self.type_defs[slot_index] = TypeDef::optional(inner_bc);
                Ok(())
            }

            TypeShape::Array { element, non_empty } => {
                let element_bc = self.resolve_type(*element, ctx.type_ctx)?;
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
                    let field_name = ctx.strings.get_or_intern(*field_sym, ctx.interner)?;
                    let field_type = self.resolve_field_type(field_info, ctx.type_ctx)?;
                    resolved_fields.push((field_name, field_type));
                }

                let member_start = self.type_members.len() as u16;
                for (field_name, field_type) in resolved_fields {
                    self.type_members
                        .push(TypeMember::new(field_name, field_type));
                }

                let member_count = u8::try_from(fields.len())
                    .map_err(|_| EmitError::TooManyFields(fields.len()))?;
                self.type_defs[slot_index] = TypeDef::for_struct(member_start, member_count);
                Ok(())
            }

            TypeShape::Enum(variants) => {
                // Resolve variant types (this may create types at later indices)
                let mut resolved_variants = Vec::with_capacity(variants.len());
                for (variant_sym, variant_type_id) in variants {
                    let variant_name = ctx.strings.get_or_intern(*variant_sym, ctx.interner)?;
                    let variant_type = self.resolve_type(*variant_type_id, ctx.type_ctx)?;
                    resolved_variants.push((variant_name, variant_type));
                }

                let member_start = self.type_members.len() as u16;
                for (variant_name, variant_type) in resolved_variants {
                    self.type_members
                        .push(TypeMember::new(variant_name, variant_type));
                }

                let member_count = u8::try_from(variants.len())
                    .map_err(|_| EmitError::TooManyVariants(variants.len()))?;
                self.type_defs[slot_index] = TypeDef::for_enum(member_start, member_count);
                Ok(())
            }

            TypeShape::Ref(def_id) => {
                let target = ctx
                    .type_ctx
                    .def_output(*def_id)
                    .expect("alias def target must exist");
                self.type_defs[slot_index] =
                    TypeDef::alias(self.resolve_type(target, ctx.type_ctx)?);
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
        type_ctx: &TypeAnalysis,
    ) -> Result<BytecodeTypeId, EmitError> {
        let type_id = self.resolve_underlying_type_id(type_id, type_ctx);
        let bc_id = self
            .mapping
            .get(&type_id)
            .copied()
            .expect("resolved type must be mapped");
        Ok(bc_id)
    }

    fn resolve_underlying_type_id(&self, type_id: TypeId, type_ctx: &TypeAnalysis) -> TypeId {
        let Some(TypeShape::Ref(def_id)) = type_ctx.type_shape(type_id) else {
            return type_id;
        };

        let target = type_ctx
            .def_output(*def_id)
            .expect("ref target def type must exist");
        self.resolve_underlying_type_id(target, type_ctx)
    }

    fn resolve_field_type(
        &mut self,
        field_info: &FieldInfo,
        type_ctx: &TypeAnalysis,
    ) -> Result<BytecodeTypeId, EmitError> {
        let base_type = self.resolve_type(field_info.type_id, type_ctx)?;

        // `Optional` is idempotent. A captured optional whose base already carries
        // `Optional` — `(Inner)? @x`, where `make_flow_optional` wrapped the inner
        // before the capture set `optional` too — must not become
        // `Optional(Optional(T))`: one skip path emits one `Null`, so the field is a
        // single `| null`.
        if field_info.optional && !self.source_is_optional(field_info.type_id, type_ctx) {
            self.get_or_create_optional(base_type)
        } else {
            Ok(base_type)
        }
    }

    fn source_is_optional(&self, type_id: TypeId, type_ctx: &TypeAnalysis) -> bool {
        let underlying = self.resolve_underlying_type_id(type_id, type_ctx);
        matches!(type_ctx.type_shape(underlying), Some(TypeShape::Optional(_)))
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

    pub fn lookup(&self, type_id: TypeId) -> Option<BytecodeTypeId> {
        self.mapping.get(&type_id).copied()
    }

    /// Get the absolute member base index for a struct/enum type.
    ///
    /// For Struct and Enum types, returns the starting index in the TypeMembers table.
    /// Fields/variants are at indices [base..base+count).
    pub fn get_member_base(&self, type_id: TypeId) -> Option<u16> {
        let bc_type_id = self.mapping.get(&type_id)?;
        let type_def = self.type_defs.get(bc_type_id.0 as usize)?;
        match type_def.decode() {
            TypeDefKind::Struct { member_start, .. } | TypeDefKind::Enum { member_start, .. } => {
                Some(member_start)
            }
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

        let mut names_bytes = Vec::with_capacity(self.type_names.len() * TypeNameEntry::SIZE);
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

struct TypeEmitSlots<'a> {
    type_ctx: &'a TypeAnalysis,
    interner: &'a Interner,
    strings: &'a mut StringTableBuilder,
}

/// Depth-first collector for custom types reachable from definition results.
/// `out` preserves the post-order (children before self) the emitter relies on;
/// `seen` guards against revisiting shared sub-types and cycles.
struct TypeCollector {
    out: Vec<TypeId>,
    seen: HashSet<TypeId>,
}

impl TypeCollector {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn collect(&mut self, type_id: TypeId, type_ctx: &TypeAnalysis) {
        if type_id.is_builtin() || self.seen.contains(&type_id) {
            return;
        }

        let Some(type_shape) = type_ctx.type_shape(type_id) else {
            return;
        };

        if let TypeShape::Ref(def_id) = type_shape {
            if let Some(target_id) = type_ctx.def_output(*def_id) {
                self.collect(target_id, type_ctx);
            }
            return;
        }

        self.seen.insert(type_id);

        match type_shape {
            TypeShape::Struct(fields) => {
                for field_info in fields.values() {
                    self.collect(field_info.type_id, type_ctx);
                }
                self.out.push(type_id);
            }
            TypeShape::Enum(variants) => {
                for &variant_type_id in variants.values() {
                    self.collect(variant_type_id, type_ctx);
                }
                self.out.push(type_id);
            }
            TypeShape::Array { element, .. } => {
                self.collect(*element, type_ctx);
                self.out.push(type_id);
            }
            TypeShape::Optional(inner) => {
                self.collect(*inner, type_ctx);
                self.out.push(type_id);
            }
            TypeShape::Custom(_) => {
                // Custom types alias Node, no children to collect
                self.out.push(type_id);
            }
            _ => {}
        }
    }
}

struct BuiltinUses {
    uses_void: bool,
    uses_node: bool,
    seen: HashSet<TypeId>,
}

impl BuiltinUses {
    fn new() -> Self {
        Self {
            uses_void: false,
            uses_node: false,
            seen: HashSet::new(),
        }
    }

    fn collect(&mut self, type_id: TypeId, type_ctx: &TypeAnalysis) {
        if !self.seen.insert(type_id) {
            return;
        }

        let Some(type_shape) = type_ctx.type_shape(type_id) else {
            return;
        };

        match type_shape {
            TypeShape::Void => self.uses_void = true,
            TypeShape::Node => self.uses_node = true,
            TypeShape::Custom(_) => self.uses_node = true, // Custom types alias Node
            TypeShape::Struct(fields) => {
                for field_info in fields.values() {
                    self.collect(field_info.type_id, type_ctx);
                }
            }
            TypeShape::Enum(variants) => {
                for &variant_type_id in variants.values() {
                    self.collect(variant_type_id, type_ctx);
                }
            }
            TypeShape::Array { element, .. } => {
                self.collect(*element, type_ctx);
            }
            TypeShape::Optional(inner) => {
                self.collect(*inner, type_ctx);
            }
            TypeShape::Ref(def_id) => {
                if let Some(target_id) = type_ctx.def_output(*def_id) {
                    self.collect(target_id, type_ctx);
                }
            }
        }
    }
}
