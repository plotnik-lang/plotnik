//! Type-table emission phase: walk the inferred types (TypeAnalysis) and lower
//! them into the bytecode type table, interning their names into the shared
//! string table. The table storage and its read accessors live in
//! `emit::tables`; this module owns the walk.

use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeKind, TypeMember, TypeNameEntry};

use crate::compiler::analyze::output::{CaptureLayout, CaptureScopeKind, OutputSchema};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TYPE_VOID, TypeShape,
};
use crate::compiler::emit::targets::bytecode::tables::{
    EmitError, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::ids::TypeId;
use crate::core::Interner;

/// Build the type table, interning type, member, and name strings into the
/// shared string table. Threads the string table by value because it extends it.
pub fn build_type_table(
    schema: &OutputSchema<'_>,
    mut strings: StringTableBuilder,
) -> Result<(TypeTableBuilder, StringTableBuilder), EmitError> {
    let mut types = TypeTableBuilder::new();
    build(&mut types, schema, &mut strings)?;
    Ok((types, strings))
}

/// Build the type table, remapping query TypeIds to bytecode ids.
///
/// Types reachable from selectable definition outputs are emitted. This keeps
/// demanded fragment capture scopes available to matcher bodies without
/// shipping unused fragments. Dead inference intermediates — a union
/// alternation's per-branch merge structs, for instance — are also pruned.
/// Used builtins lead, then custom types follow in definition order,
/// depth-first.
fn build(
    types: &mut TypeTableBuilder,
    schema: &OutputSchema<'_>,
    strings: &mut StringTableBuilder,
) -> Result<(), EmitError> {
    let type_analysis = schema.types;
    let type_layout = schema.type_layout();
    let ordered_types = type_layout.custom_types();

    emit_builtins(types, type_layout.builtins())?;
    reserve_slots(types, ordered_types)?;

    let mut ctx = TypeEmitCtx {
        type_analysis,
        interner: schema.interner,
        strings,
    };
    fill_slots(types, ordered_types, schema.layout(), &mut ctx)?;
    assert_eq!(
        usize::from(types.members_len()),
        schema.layout().member_count(),
        "wire type members consume every shared capture slot"
    );
    emit_type_names(types, schema, &mut ctx)?;

    Ok(())
}

fn emit_builtins(types: &mut TypeTableBuilder, builtins: &[TypeId]) -> Result<(), EmitError> {
    for &builtin in builtins {
        let kind = match builtin {
            TYPE_VOID => TypeKind::Void,
            TYPE_NODE => TypeKind::Node,
            TYPE_TEXT => TypeKind::Text,
            TYPE_BOOL => TypeKind::Bool,
            _ => unreachable!("output type layout exposes only primitive built-ins"),
        };
        types.push_mapped(builtin, TypeDef::builtin(kind))?;
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
    layout: &CaptureLayout,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    for &type_id in ordered_types {
        emit_type_at_slot(types, type_id, layout, ctx)?;
    }
    Ok(())
}

fn emit_type_names(
    types: &mut TypeTableBuilder,
    schema: &OutputSchema<'_>,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    // The naming pass is the single source of names: definition results,
    // path-generated composites, and custom `:: TypeName` capture types, in `TypeId`
    // (deterministic) order. Naming covers the whole analyzed query; output
    // layout deliberately keeps only names whose types survive selectable
    // definition roots
    // reachability.
    for (type_id, name_sym) in schema.iter_type_names() {
        if !schema.type_layout().contains(type_id) {
            continue;
        }
        let bc_type_id = types.lookup(type_id).expect("named type must be mapped");
        let name = ctx.strings.intern(name_sym, ctx.interner)?;
        types.push_name(TypeNameEntry::new(name, bc_type_id))?;
    }

    Ok(())
}

fn emit_type_at_slot(
    types: &mut TypeTableBuilder,
    type_id: TypeId,
    layout: &CaptureLayout,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    let wire_type = types
        .lookup(type_id)
        .expect("reserved output type has a wire slot");
    let slot_index = usize::from(u16::from(wire_type));
    let type_shape = ctx.type_analysis.expect_type_shape(type_id);
    match type_shape {
        TypeShape::Void | TypeShape::Node | TypeShape::Text | TypeShape::Bool => {
            unreachable!("builtins should be handled separately")
        }

        TypeShape::Custom(_) => {
            // A custom capture type is a nominal alias for Node.
            // The name entry comes from the naming pass via `emit_type_names`;
            // here only the alias shape is emitted.
            let node = types
                .lookup(TYPE_NODE)
                .expect("Node is mapped before aliases are emitted");
            types.fill_slot(slot_index, TypeDef::alias(node));
            Ok(())
        }

        TypeShape::Option(inner) => {
            let inner_bc = types.resolve_type(*inner, ctx.type_analysis)?;
            types.fill_slot(slot_index, TypeDef::option(inner_bc));
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

        TypeShape::Record(fields) => {
            // Resolve field types (this may create Option wrappers at later indices)
            let mut resolved_fields = Vec::with_capacity(fields.len());
            for (field_sym, field_info) in fields {
                let field_name = ctx.strings.intern(*field_sym, ctx.interner)?;
                let field_type = resolve_field_type(types, field_info, ctx.type_analysis)?;
                resolved_fields.push((field_name, field_type));
            }

            let scope = layout
                .scope(type_id)
                .expect("every emitted record has a capture scope");
            assert_eq!(scope.kind(), CaptureScopeKind::Record);
            let member_start = scope.base();
            assert_eq!(
                types.members_len(),
                member_start,
                "wire members consume the shared capture layout in order"
            );
            for (field_name, field_type) in resolved_fields {
                types.push_member(TypeMember::new(field_name, field_type));
            }

            let member_count = u8::try_from(scope.members().len())
                .map_err(|_| EmitError::TooManyFields(scope.members().len()))?;
            types.fill_slot(slot_index, TypeDef::for_record(member_start, member_count));
            Ok(())
        }

        TypeShape::Variant(cases) => {
            // Resolve case types (this may create types at later indices).
            let mut resolved_cases = Vec::with_capacity(cases.len());
            for (case_sym, case_type_id) in cases {
                let case_name = ctx.strings.intern(*case_sym, ctx.interner)?;
                let case_type = types.resolve_type(*case_type_id, ctx.type_analysis)?;
                resolved_cases.push((case_name, case_type));
            }

            let scope = layout
                .scope(type_id)
                .expect("every emitted variant type has a capture scope");
            assert_eq!(scope.kind(), CaptureScopeKind::Variant);
            let member_start = scope.base();
            assert_eq!(
                types.members_len(),
                member_start,
                "wire members consume the shared capture layout in order"
            );
            for (case_name, case_type) in resolved_cases {
                types.push_member(TypeMember::new(case_name, case_type));
            }

            let member_count = u8::try_from(scope.members().len())
                .map_err(|_| EmitError::TooManyCases(scope.members().len()))?;
            types.fill_slot(slot_index, TypeDef::for_variant(member_start, member_count));
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
    field_info: &RecordField,
    type_ctx: &TypeAnalysis,
) -> Result<WireTypeId, EmitError> {
    types.resolve_type(field_info.final_type, type_ctx)
}

struct TypeEmitCtx<'a> {
    type_analysis: &'a TypeAnalysis,
    interner: &'a Interner,
    strings: &'a mut StringTableBuilder,
}
