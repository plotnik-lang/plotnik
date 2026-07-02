#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type-table emission phase: walk the inferred types (TypeAnalysis) and lower
//! them into the bytecode type table, interning their names into the shared
//! string table. The table storage and its read accessors live in
//! `emit::tables`; this module owns the walk.

use std::collections::HashSet;

use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeKind, TypeMember, TypeNameEntry};

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_NODE, TYPE_VOID, TypeShape};
use crate::compiler::emit::tables::{EmitError, StringTableBuilder, TypeTableBuilder};
use crate::compiler::ids::TypeId;
use crate::core::Interner;

/// Build the type table, interning type, member, and name strings into the
/// shared string table. Threads the string table by value because it extends it.
pub fn build_type_table(
    input: &AnalysisArtifacts<'_>,
    mut strings: StringTableBuilder,
) -> Result<(TypeTableBuilder, StringTableBuilder), EmitError> {
    let mut types = TypeTableBuilder::new();
    build(&mut types, *input, &mut strings)?;
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
    input: AnalysisArtifacts<'_>,
    strings: &mut StringTableBuilder,
) -> Result<(), EmitError> {
    let type_analysis = input.type_analysis;
    let ordered_types = collect_ordered_types(type_analysis);
    let usage = scan_builtin_usage(type_analysis, &ordered_types);

    emit_builtins(types, &usage)?;
    reserve_slots(types, &ordered_types)?;

    let mut ctx = TypeEmitCtx {
        type_analysis,
        interner: input.interner,
        strings,
    };
    fill_slots(types, &ordered_types, &usage, &mut ctx)?;
    emit_type_names(types, &input, &mut ctx)?;

    Ok(())
}

/// Collect custom types depth-first from definition result types. Every emitted
/// effect's member ref names a type that one of these reaches, so this single
/// walk also covers all effect-referenced types. Definition order fixes the
/// emission order entrypoints rely on.
fn collect_ordered_types(type_ctx: &TypeAnalysis) -> Vec<TypeId> {
    let mut collector = TypeCollector::new();
    for (_def_id, type_id) in type_ctx.iter_def_output() {
        collector.collect(type_id, type_ctx);

        if !matches!(type_ctx.expect_type_shape(type_id), TypeShape::Ref(_)) {
            continue;
        }

        if !collector.seen.insert(type_id) {
            continue;
        }

        collector.out.push(type_id);
    }

    collector.out
}

fn scan_builtin_usage(type_ctx: &TypeAnalysis, ordered_types: &[TypeId]) -> BuiltinUsage {
    let mut usage = BuiltinUsage::new();
    for &type_id in ordered_types {
        usage.collect(type_id, type_ctx);
    }

    for (_def_id, type_id) in type_ctx.iter_def_output() {
        if type_id == TYPE_VOID {
            usage.uses_void = true;
        } else if type_id == TYPE_NODE {
            usage.uses_node = true;
        }
    }

    usage
}

fn emit_builtins(types: &mut TypeTableBuilder, usage: &BuiltinUsage) -> Result<(), EmitError> {
    let builtin_types = [
        (TYPE_VOID, TypeKind::Void, usage.uses_void),
        (TYPE_NODE, TypeKind::Node, usage.uses_node),
    ];
    for &(builtin_id, kind, used) in &builtin_types {
        if used {
            types.push_mapped(builtin_id, TypeDef::builtin(kind))?;
        }
    }
    Ok(())
}

fn reserve_slots(types: &mut TypeTableBuilder, ordered_types: &[TypeId]) -> Result<(), EmitError> {
    for &type_id in ordered_types {
        types.push_mapped(type_id, TypeDef::placeholder())?;
    }
    Ok(())
}

fn fill_slots(
    types: &mut TypeTableBuilder,
    ordered_types: &[TypeId],
    usage: &BuiltinUsage,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    let builtin_count = usage.builtin_count();
    for (i, &type_id) in ordered_types.iter().enumerate() {
        let slot_index = builtin_count + i;
        let type_shape = ctx.type_analysis.expect_type_shape(type_id);
        emit_type_at_slot(types, slot_index, type_shape, ctx)?;
    }
    Ok(())
}

fn emit_type_names(
    types: &mut TypeTableBuilder,
    _input: &AnalysisArtifacts<'_>,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    // The naming pass is the single source of names: definition results,
    // path-generated composites, and `:: TypeName` annotations, in `TypeId`
    // (deterministic) order. Every named type is reachable from a definition
    // result, so it survives dead-type elimination; a miss here is a compiler
    // bug, not anything a query can trigger.
    for (type_id, name_sym) in ctx.type_analysis.iter_type_names() {
        let bc_type_id = types.lookup(type_id).expect("named type must be mapped");
        let name = ctx.strings.intern(name_sym, ctx.interner)?;
        types.push_name(TypeNameEntry::new(name, bc_type_id))?;
    }

    Ok(())
}

fn emit_type_at_slot(
    types: &mut TypeTableBuilder,
    slot_index: usize,
    type_shape: &TypeShape,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    match type_shape {
        TypeShape::Void | TypeShape::Node => {
            unreachable!("builtins should be handled separately")
        }

        TypeShape::Custom(_) => {
            // Custom type annotation: @x :: Identifier → type Identifier = Node.
            // The name entry comes from the naming pass via `emit_type_names`;
            // here only the alias shape is emitted.
            //
            // Custom types alias Node - look up Node's actual bytecode ID.
            // Reaching a Custom type means it was in `ordered_types`, so
            // `BuiltinUsage::collect` marked Node used (`Custom(_) => usage.node =
            // true`) and `emit_builtins` mapped it before custom slots are filled.
            let node_bc_id = types
                .lookup(TYPE_NODE)
                .expect("Node must be mapped before a Custom alias that targets it is emitted");
            types.fill_slot(slot_index, TypeDef::alias(node_bc_id));
            Ok(())
        }

        TypeShape::Optional(inner) => {
            let inner_bc = types.resolve_type(*inner, ctx.type_analysis)?;
            types.fill_slot(slot_index, TypeDef::optional(inner_bc));
            Ok(())
        }

        TypeShape::Array { element, non_empty } => {
            let element_bc = types.resolve_type(*element, ctx.type_analysis)?;
            let def = if *non_empty {
                TypeDef::array_plus(element_bc)
            } else {
                TypeDef::array_star(element_bc)
            };
            types.fill_slot(slot_index, def);
            Ok(())
        }

        TypeShape::Struct(fields) => {
            // Resolve field types (this may create Optional wrappers at later indices)
            let mut resolved_fields = Vec::with_capacity(fields.len());
            for (field_sym, field_info) in fields {
                let field_name = ctx.strings.intern(*field_sym, ctx.interner)?;
                let field_type = resolve_field_type(types, field_info, ctx.type_analysis)?;
                resolved_fields.push((field_name, field_type));
            }

            let member_start = types.members_len()?;
            for (field_name, field_type) in resolved_fields {
                types.push_member(TypeMember::new(field_name, field_type))?;
            }

            if fields.len() > EmitError::MAX_FIELDS {
                return Err(EmitError::TooManyFields(fields.len()));
            }
            let member_count = fields.len() as u8;
            types.fill_slot(slot_index, TypeDef::for_struct(member_start, member_count));
            Ok(())
        }

        TypeShape::Enum(variants) => {
            // Resolve variant types (this may create types at later indices)
            let mut resolved_variants = Vec::with_capacity(variants.len());
            for (variant_sym, variant_type_id) in variants {
                let variant_name = ctx.strings.intern(*variant_sym, ctx.interner)?;
                let variant_type = types.resolve_type(*variant_type_id, ctx.type_analysis)?;
                resolved_variants.push((variant_name, variant_type));
            }

            let member_start = types.members_len()?;
            for (variant_name, variant_type) in resolved_variants {
                types.push_member(TypeMember::new(variant_name, variant_type))?;
            }

            if variants.len() > EmitError::MAX_VARIANTS {
                return Err(EmitError::TooManyVariants(variants.len()));
            }
            let member_count = variants.len() as u8;
            types.fill_slot(slot_index, TypeDef::for_enum(member_start, member_count));
            Ok(())
        }

        TypeShape::Ref(def_id) => {
            // A recursive reference to a definition that ended up void (its
            // captures all suppressed) leaves no pending value at runtime: the
            // capture takes the matched node, so the alias targets Node.
            let target = ctx.type_analysis.expect_def_output(*def_id);
            let alias = if target == TYPE_VOID {
                types
                    .lookup(TYPE_NODE)
                    .expect("Node is mapped before a Ref alias that targets it is emitted")
            } else {
                types.resolve_type(target, ctx.type_analysis)?
            };
            types.fill_slot(slot_index, TypeDef::alias(alias));
            Ok(())
        }
    }
}

fn resolve_field_type(
    types: &mut TypeTableBuilder,
    field_info: &FieldInfo,
    type_ctx: &TypeAnalysis,
) -> Result<WireTypeId, EmitError> {
    let base_type = types.resolve_type(field_info.type_id, type_ctx)?;

    if field_info.optional {
        // Field optionality is single-sourced: inference puts a captured `?`'s
        // null on the capture field alone, never on the base type as well. A
        // base still resolving to `Optional` would declare the null twice for
        // one skip-path `Null`.
        assert!(
            !source_is_optional(type_ctx, field_info.type_id),
            "optional field over an already-optional base type"
        );
        types.intern_optional(base_type)
    } else {
        Ok(base_type)
    }
}

fn source_is_optional(type_ctx: &TypeAnalysis, type_id: TypeId) -> bool {
    let underlying = type_ctx.resolve_underlying_type_id(type_id);
    matches!(
        type_ctx.expect_type_shape(underlying),
        TypeShape::Optional(_)
    )
}

struct TypeEmitCtx<'a> {
    type_analysis: &'a TypeAnalysis,
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

        let type_shape = type_ctx.expect_type_shape(type_id);

        if let TypeShape::Ref(def_id) = type_shape {
            let target_id = type_ctx.expect_def_output(*def_id);
            self.collect(target_id, type_ctx);
            return;
        }

        self.seen.insert(type_id);

        for child_id in type_shape.child_type_ids() {
            self.collect(child_id, type_ctx);
        }

        match type_shape {
            TypeShape::Struct(_)
            | TypeShape::Enum(_)
            | TypeShape::Array { .. }
            | TypeShape::Optional(_)
            | TypeShape::Custom(_) => self.out.push(type_id),
            _ => {}
        }
    }
}

struct BuiltinUsage {
    uses_void: bool,
    uses_node: bool,
    seen: HashSet<TypeId>,
}

impl BuiltinUsage {
    fn new() -> Self {
        Self {
            uses_void: false,
            uses_node: false,
            seen: HashSet::new(),
        }
    }

    fn builtin_count(&self) -> usize {
        self.uses_void as usize + self.uses_node as usize
    }

    fn collect(&mut self, type_id: TypeId, type_ctx: &TypeAnalysis) {
        if !self.seen.insert(type_id) {
            return;
        }

        let type_shape = type_ctx.expect_type_shape(type_id);

        match type_shape {
            TypeShape::Void => self.uses_void = true,
            TypeShape::Node => self.uses_node = true,
            TypeShape::Custom(_) => self.uses_node = true, // Custom types alias Node
            TypeShape::Ref(def_id) => {
                let target_id = type_ctx.expect_def_output(*def_id);
                if target_id == TYPE_VOID {
                    // The Ref emits as an alias of Node (see `emit_type_at_slot`).
                    self.uses_node = true;
                } else {
                    self.collect(target_id, type_ctx);
                }
            }
            _ => {
                for child_id in type_shape.child_type_ids() {
                    self.collect(child_id, type_ctx);
                }
            }
        }
    }
}
