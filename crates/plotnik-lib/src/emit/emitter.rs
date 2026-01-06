//! Core bytecode emission logic.
//!
//! Contains the main entry points for emitting bytecode from compiled queries.

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};

use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{TypeContext, TypeId};
use crate::bytecode::Label;
use crate::bytecode::{
    Entrypoint, FieldSymbol, Header, NodeSymbol, SECTION_ALIGN, TriviaEntry, TypeMetaHeader,
};
use crate::compile::Compiler;
use crate::query::LinkedQuery;

use super::EmitError;
use super::layout::CacheAligned;
use super::string_table::StringTableBuilder;
use super::type_table::TypeTableBuilder;

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
    let compile_result = Compiler::compile(
        interner,
        type_ctx,
        symbol_table,
        &mut strings,
        node_type_ids,
        node_field_ids,
    )
    .map_err(EmitError::Compile)?;

    // Layout with cache alignment
    // Preamble entry FIRST ensures it gets the lowest address (step 0)
    let mut entry_labels: Vec<Label> = vec![compile_result.preamble_entry];
    entry_labels.extend(compile_result.def_entries.values().copied());
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
        let result_type = types.resolve_type(type_id, type_ctx)?;

        // Get actual target from compiled result
        let target = compile_result
            .def_entries
            .get(&def_id)
            .and_then(|label| layout.label_to_step.get(label))
            .copied()
            .expect("entrypoint must have compiled target");

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
    let transitions_bytes =
        emit_transitions(&compile_result.instructions, &layout, &types, &strings);

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

/// Pad a buffer to the section alignment boundary.
fn pad_to_section(buf: &mut Vec<u8>) {
    let rem = buf.len() % SECTION_ALIGN;
    if rem != 0 {
        let padding = SECTION_ALIGN - rem;
        buf.resize(buf.len() + padding, 0);
    }
}

/// Emit transitions section from instructions and layout.
fn emit_transitions(
    instructions: &[crate::bytecode::InstructionIR],
    layout: &crate::bytecode::LayoutResult,
    types: &TypeTableBuilder,
    strings: &StringTableBuilder,
) -> Vec<u8> {
    // Allocate buffer for all steps (8 bytes each)
    let mut bytes = vec![0u8; layout.total_steps as usize * 8];

    // Create resolver closures for member indices.
    // lookup_member: for struct fields (deduplicated by field identity)
    // get_member_base: for enum variants (parent_type + relative_index)
    let lookup_member = |field_name: Symbol, field_type: TypeId| {
        types.lookup_member(field_name, field_type, strings)
    };
    let get_member_base = |type_id: TypeId| types.get_member_base(type_id);

    for instr in instructions {
        let label = instr.label();
        let Some(&step_id) = layout.label_to_step.get(&label) else {
            continue;
        };

        let offset = step_id as usize * 8; // STEP_SIZE
        let resolved = instr.resolve(&layout.label_to_step, lookup_member, get_member_base);

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
        bytes.extend_from_slice(&sym.name.get().to_le_bytes());
    }
    bytes
}

fn emit_field_symbols(symbols: &[FieldSymbol]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * 4);
    for sym in symbols {
        bytes.extend_from_slice(&sym.id.to_le_bytes());
        bytes.extend_from_slice(&sym.name.get().to_le_bytes());
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
        bytes.extend_from_slice(&ep.name.get().to_le_bytes());
        bytes.extend_from_slice(&ep.target.to_le_bytes());
        bytes.extend_from_slice(&ep.result_type.0.to_le_bytes());
        bytes.extend_from_slice(&ep._pad.to_le_bytes());
    }
    bytes
}
