#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type-table emission phase: walk the inferred types (TypeAnalysis) and lower
//! them into the bytecode type table, interning their names into the shared
//! string table. The table storage and its read accessors live in
//! `emit::tables`; this module owns the walk.

use std::collections::HashSet;

use crate::bytecode::{TypeDef, TypeId as BytecodeTypeId, TypeKind, TypeMember, TypeNameEntry};

use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_NODE, TYPE_VOID, TypeShape};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::core::{Interner, TypeId};
use crate::compiler::emit::tables::{EmitError, EmitInput, StringTableBuilder, TypeTableBuilder};

/// Build the type table, interning type, member, and name strings into the
/// shared string table. Threads the string table by value because it extends it.
pub fn build_type_table(
    input: &EmitInput<'_>,
    mut strings: StringTableBuilder,
) -> Result<(TypeTableBuilder, StringTableBuilder), EmitError> {
    let mut types = TypeTableBuilder::new();
    build(
        &mut types,
        input.type_ctx,
        input.dependency_analysis,
        input.interner,
        &mut strings,
    )?;
    Ok((types, strings))
}

/// Build the type table, remapping query TypeIds to bytecode ids.
///
/// Only types reachable from an entrypoint result are emitted. Dead
/// intermediate types produced during inference — a union alternation's
/// per-branch merge structs, for instance — are pruned. Used builtins are
/// emitted first, then custom types in definition order, depth-first.
fn build(
    types: &mut TypeTableBuilder,
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
            types.push_mapped(builtin_id, TypeDef::builtin(kind));
        }
    }

    // Phase 2: Pre-assign BytecodeTypeIds for custom types and reserve slots
    for &type_id in &ordered_types {
        types.push_mapped(type_id, TypeDef::placeholder());
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
        emit_type_at_slot(types, slot_index, type_shape, &mut ctx)?;
    }

    for (def_id, type_id) in type_ctx.iter_def_output() {
        let name_sym = dependency_analysis.def_name_sym(def_id);
        let name = ctx.strings.get_or_intern(name_sym, ctx.interner)?;
        let bc_type_id = types
            .lookup(type_id)
            .expect("def result type must be mapped");
        types.push_name(TypeNameEntry::new(name, bc_type_id));
    }

    // Collect TypeNameEntry entries for explicit type annotations on struct captures,
    // e.g. `{(fn) @fn} @outer :: FunctionInfo` names the struct "FunctionInfo".
    // A name only attaches to a non-suppressive capture's struct/enum, so the
    // type is reachable from a def result and must survive dead-type elimination;
    // a miss here is a compiler bug, not anything a query can trigger.
    for (type_id, name_sym) in type_ctx.iter_type_aliases() {
        let bc_type_id = types
            .lookup(type_id)
            .expect("named type annotation must survive dead-type elimination");
        let name = ctx.strings.get_or_intern(name_sym, ctx.interner)?;
        types.push_name(TypeNameEntry::new(name, bc_type_id));
    }

    Ok(())
}

fn emit_type_at_slot(
    types: &mut TypeTableBuilder,
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
            types.push_name(TypeNameEntry::new(name, bc_type_id));

            // Custom types alias Node - look up Node's actual bytecode ID.
            // Reaching a Custom type means it was in `ordered_types`, so
            // `BuiltinUses::collect` marked Node used (`Custom(_) => usage.node =
            // true`) and Phase 1 emitted it into `mapping`.
            let node_bc_id = types
                .lookup(TYPE_NODE)
                .expect("Node must be mapped before a Custom alias that targets it is emitted");
            types.set_type_def(slot_index, TypeDef::alias(node_bc_id));
            Ok(())
        }

        TypeShape::Optional(inner) => {
            let inner_bc = types.resolve_type(*inner, ctx.type_ctx)?;
            types.set_type_def(slot_index, TypeDef::optional(inner_bc));
            Ok(())
        }

        TypeShape::Array { element, non_empty } => {
            let element_bc = types.resolve_type(*element, ctx.type_ctx)?;
            let def = if *non_empty {
                TypeDef::array_plus(element_bc)
            } else {
                TypeDef::array_star(element_bc)
            };
            types.set_type_def(slot_index, def);
            Ok(())
        }

        TypeShape::Struct(fields) => {
            // Resolve field types (this may create Optional wrappers at later indices)
            let mut resolved_fields = Vec::with_capacity(fields.len());
            for (field_sym, field_info) in fields {
                let field_name = ctx.strings.get_or_intern(*field_sym, ctx.interner)?;
                let field_type = resolve_field_type(types, field_info, ctx.type_ctx)?;
                resolved_fields.push((field_name, field_type));
            }

            let member_start = types.members_len();
            for (field_name, field_type) in resolved_fields {
                types.push_member(TypeMember::new(field_name, field_type));
            }

            let member_count =
                u8::try_from(fields.len()).map_err(|_| EmitError::TooManyFields(fields.len()))?;
            types.set_type_def(slot_index, TypeDef::for_struct(member_start, member_count));
            Ok(())
        }

        TypeShape::Enum(variants) => {
            // Resolve variant types (this may create types at later indices)
            let mut resolved_variants = Vec::with_capacity(variants.len());
            for (variant_sym, variant_type_id) in variants {
                let variant_name = ctx.strings.get_or_intern(*variant_sym, ctx.interner)?;
                let variant_type = types.resolve_type(*variant_type_id, ctx.type_ctx)?;
                resolved_variants.push((variant_name, variant_type));
            }

            let member_start = types.members_len();
            for (variant_name, variant_type) in resolved_variants {
                types.push_member(TypeMember::new(variant_name, variant_type));
            }

            let member_count = u8::try_from(variants.len())
                .map_err(|_| EmitError::TooManyVariants(variants.len()))?;
            types.set_type_def(slot_index, TypeDef::for_enum(member_start, member_count));
            Ok(())
        }

        TypeShape::Ref(def_id) => {
            let target = ctx
                .type_ctx
                .def_output(*def_id)
                .expect("alias def target must exist");
            let alias = types.resolve_type(target, ctx.type_ctx)?;
            types.set_type_def(slot_index, TypeDef::alias(alias));
            Ok(())
        }
    }
}

fn resolve_field_type(
    types: &mut TypeTableBuilder,
    field_info: &FieldInfo,
    type_ctx: &TypeAnalysis,
) -> Result<BytecodeTypeId, EmitError> {
    let base_type = types.resolve_type(field_info.type_id, type_ctx)?;

    // `Optional` is idempotent. A captured optional whose base already carries
    // `Optional` — `(Inner)? @x`, where `make_flow_optional` wrapped the inner
    // before the capture set `optional` too — must not become
    // `Optional(Optional(T))`: one skip path emits one `Null`, so the field is a
    // single `| null`.
    if field_info.optional && !source_is_optional(type_ctx, field_info.type_id) {
        types.get_or_create_optional(base_type)
    } else {
        Ok(base_type)
    }
}

fn source_is_optional(type_ctx: &TypeAnalysis, type_id: TypeId) -> bool {
    let underlying = type_ctx.resolve_underlying_type_id(type_id);
    matches!(
        type_ctx.type_shape(underlying),
        Some(TypeShape::Optional(_))
    )
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
