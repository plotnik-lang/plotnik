//! Type-table emission phase: walk the inferred types (TypeAnalysis) and lower
//! them into the bytecode type table, interning their names into the shared
//! string table. The table storage and its read accessors live in
//! `emit::tables`; this module owns the walk.

use crate::bytecode::{TypeDef, TypeId as WireTypeId, TypeKind, TypeMember, TypeNameEntry};

use crate::compiler::analyze::result::{CaptureLayout, CaptureScopeKind, ResultSchema};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    DefinitionOutput, ListMinimum, RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TypeShape,
};
use crate::compiler::emit::targets::bytecode::tables::{
    EmitError, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::ids::TypeId;
use crate::core::Interner;

/// Build the type table, interning type, member, and name strings into the
/// shared string table. Threads the string table by value because it extends it.
pub fn build_type_table(
    schema: &ResultSchema<'_>,
    mut strings: StringTableBuilder,
) -> Result<(TypeTableBuilder, StringTableBuilder), EmitError> {
    let mut types = TypeTableBuilder::new();
    build(&mut types, schema, &mut strings)?;
    Ok((types, strings))
}

/// Build the type table, remapping analysis type IDs to bytecode type IDs.
///
/// Types reachable from selectable definition outputs are emitted. This keeps
/// demanded fragment capture scopes available to matcher bodies without
/// shipping unused fragments. Dead inference intermediates — an unlabeled
/// alternation's per-alternative merged records, for instance — are also pruned.
/// Used built-ins lead, then value types follow in definition order,
/// depth-first.
fn build(
    types: &mut TypeTableBuilder,
    schema: &ResultSchema<'_>,
    strings: &mut StringTableBuilder,
) -> Result<(), EmitError> {
    let type_analysis = schema.types;
    let type_layout = schema.type_layout();
    let ordered_types = type_layout.value_types();

    emit_builtins(types, type_layout)?;
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

fn emit_builtins(
    types: &mut TypeTableBuilder,
    layout: &crate::compiler::analyze::result::ResultTypeLayout,
) -> Result<(), EmitError> {
    if layout.has_no_value() {
        types.push_no_value()?;
    }
    for &builtin in layout.builtins() {
        let kind = match builtin {
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
    schema: &ResultSchema<'_>,
    ctx: &mut TypeEmitCtx,
) -> Result<(), EmitError> {
    for binding in schema.iter_type_name_bindings() {
        if !schema.type_layout().contains(binding.type_id) {
            continue;
        }
        let bc_type_id = types
            .lookup(binding.type_id)
            .expect("named output type must be mapped");
        let name = ctx.strings.intern(binding.name, ctx.interner)?;
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
        TypeShape::Node | TypeShape::Text | TypeShape::Bool => {
            unreachable!("builtins should be handled separately")
        }

        TypeShape::Option(inner) => {
            let inner_bc = types.resolve_type(*inner, ctx.type_analysis)?;
            types.fill_slot(slot_index, TypeDef::option(inner_bc));
            Ok(())
        }

        TypeShape::List { element, minimum } => {
            let element_bc = types.resolve_type(*element, ctx.type_analysis)?;
            let def = match minimum {
                ListMinimum::Zero => TypeDef::list_zero_or_more(element_bc),
                ListMinimum::One => TypeDef::list_one_or_more(element_bc),
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
            for (case_sym, payload) in cases {
                let case_name = ctx.strings.intern(*case_sym, ctx.interner)?;
                let case_type = match payload.type_id() {
                    Some(type_id) => types.resolve_type(type_id, ctx.type_analysis)?,
                    None => types.resolve_output(DefinitionOutput::MatchOnly, ctx.type_analysis)?,
                };
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

        TypeShape::Ref(declaration) => {
            let alias = match ctx.type_analysis.declaration_body(*declaration) {
                None => types
                    .lookup(TYPE_NODE)
                    .expect("Node is mapped before a Ref alias that targets it is emitted"),
                Some(type_id) => types.resolve_type(type_id, ctx.type_analysis)?,
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
