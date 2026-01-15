//! Core bytecode emission logic.

use std::cell::RefCell;

use plotnik_core::Symbol;

use crate::analyze::type_check::TypeId;
use crate::bytecode::{InstructionIR, Label, PredicateValueIR};
use crate::compile::{CompileCtx, Compiler};
use crate::query::LinkedQuery;
use plotnik_bytecode::{Entrypoint, FieldSymbol, Header, NodeSymbol, SECTION_ALIGN, TriviaEntry};

use super::EmitError;
use super::layout::CacheAligned;
use super::regex_table::RegexTableBuilder;
use super::string_table::StringTableBuilder;
use super::type_table::TypeTableBuilder;

/// Emit bytecode from a LinkedQuery.
pub fn emit(query: &LinkedQuery) -> Result<Vec<u8>, EmitError> {
    let type_ctx = query.type_context();
    let interner = query.interner();
    let symbol_table = &query.symbol_table;
    let node_type_ids = query.node_type_ids();
    let node_field_ids = query.node_field_ids();

    let strings = RefCell::new(StringTableBuilder::new());
    let mut types = TypeTableBuilder::new();
    types.build(type_ctx, interner, &mut strings.borrow_mut())?;

    let ctx = CompileCtx {
        interner,
        type_ctx,
        symbol_table,
        strings: &strings,
        node_types: Some(node_type_ids),
        node_fields: Some(node_field_ids),
    };
    let compile_result = Compiler::compile(&ctx).map_err(EmitError::Compile)?;

    // Layout with cache alignment
    // Preamble entry FIRST ensures it gets the lowest address (step 0)
    let mut entry_labels: Vec<Label> = vec![compile_result.preamble_entry];
    entry_labels.extend(compile_result.def_entries.values().copied());
    let layout = CacheAligned::layout(&compile_result.instructions, &entry_labels);

    // Validate transition count
    if layout.total_steps as usize > 65535 {
        return Err(EmitError::TooManyTransitions(layout.total_steps as usize));
    }

    // Collect node symbols
    let mut node_symbols: Vec<NodeSymbol> = Vec::new();
    for (&sym, &node_id) in node_type_ids {
        let name = strings.borrow_mut().get_or_intern(sym, interner)?;
        node_symbols.push(NodeSymbol::new(node_id.get(), name));
    }

    // Collect field symbols
    let mut field_symbols: Vec<FieldSymbol> = Vec::new();
    for (&sym, &field_id) in node_field_ids {
        let name = strings.borrow_mut().get_or_intern(sym, interner)?;
        field_symbols.push(FieldSymbol::new(field_id.get(), name));
    }

    // Collect entrypoints with actual targets from layout
    let mut entrypoints: Vec<Entrypoint> = Vec::new();
    for (def_id, type_id) in type_ctx.iter_def_types() {
        let name_sym = type_ctx.def_name_sym(def_id);
        let name = strings.borrow_mut().get_or_intern(name_sym, interner)?;
        let result_type = types.resolve_type(type_id, type_ctx)?;

        // Get actual target from compiled result
        let target = compile_result
            .def_entries
            .get(&def_id)
            .and_then(|label| layout.label_to_step().get(label))
            .copied()
            .expect("entrypoint must have compiled target");

        entrypoints.push(Entrypoint::new(name, target, result_type));
    }

    // Move strings out of RefCell for final emission
    let strings = strings.into_inner();

    // Validate counts
    strings.validate()?;
    types.validate()?;
    if entrypoints.len() > 65535 {
        return Err(EmitError::TooManyEntrypoints(entrypoints.len()));
    }

    // Trivia (empty for now)
    let trivia_entries: Vec<TriviaEntry> = Vec::new();

    // Build regex table from predicates in compiled instructions
    let mut regexes = RegexTableBuilder::new();
    intern_regex_predicates(&compile_result.instructions, &strings, &mut regexes)?;
    regexes.validate()?;

    // Resolve and serialize transitions
    let transitions_bytes = emit_transitions(
        &compile_result.instructions,
        &layout,
        &types,
        &strings,
        &regexes,
    );

    // Emit all byte sections
    let (str_blob, str_table) = strings.emit();
    let (regex_blob, regex_table) = regexes.emit();
    let (type_defs_bytes, type_members_bytes, type_names_bytes) = types.emit();

    let node_types_bytes = emit_node_symbols(&node_symbols);
    let node_fields_bytes = emit_field_symbols(&field_symbols);
    let trivia_bytes = emit_trivia(&trivia_entries);
    let entrypoints_bytes = emit_entrypoints(&entrypoints);

    // Build output with sections in v2 order:
    // Header → StringBlob → RegexBlob → StringTable → RegexTable →
    // NodeTypes → NodeFields → Trivia → TypeDefs → TypeMembers →
    // TypeNames → Entrypoints → Transitions
    let mut output = vec![0u8; 64]; // Reserve header space

    emit_section(&mut output, &str_blob);
    emit_section(&mut output, &regex_blob);
    emit_section(&mut output, &str_table);
    emit_section(&mut output, &regex_table);
    emit_section(&mut output, &node_types_bytes);
    emit_section(&mut output, &node_fields_bytes);
    emit_section(&mut output, &trivia_bytes);
    emit_section(&mut output, &type_defs_bytes);
    emit_section(&mut output, &type_members_bytes);
    emit_section(&mut output, &type_names_bytes);
    emit_section(&mut output, &entrypoints_bytes);
    emit_section(&mut output, &transitions_bytes);

    pad_to_section(&mut output);
    let total_size = output.len() as u32;

    // Build header (offsets computed from counts and blob sizes)
    let mut header = Header {
        str_table_count: strings.len() as u16,
        node_types_count: node_symbols.len() as u16,
        node_fields_count: field_symbols.len() as u16,
        trivia_count: trivia_entries.len() as u16,
        regex_table_count: regexes.len() as u16,
        type_defs_count: types.type_defs_count() as u16,
        type_members_count: types.type_members_count() as u16,
        type_names_count: types.type_names_count() as u16,
        entrypoints_count: entrypoints.len() as u16,
        transitions_count: layout.total_steps,
        str_blob_size: str_blob.len() as u32,
        regex_blob_size: regex_blob.len() as u32,
        total_size,
        ..Default::default()
    };
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
    regexes: &RegexTableBuilder,
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

    // Predicate regex resolution closure.
    let lookup_regex = |string_id: plotnik_bytecode::StringId| regexes.get(string_id);

    for instr in instructions {
        let label = instr.label();
        let Some(&step_id) = layout.label_to_step.get(&label) else {
            continue;
        };

        let offset = step_id as usize * 8; // STEP_SIZE
        let resolved = instr.resolve(
            &layout.label_to_step,
            lookup_member,
            get_member_base,
            lookup_regex,
        );

        // Copy instruction bytes to the correct position
        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    bytes
}

/// Pre-scan instructions for regex predicates and intern them.
fn intern_regex_predicates(
    instructions: &[InstructionIR],
    strings: &StringTableBuilder,
    regexes: &mut RegexTableBuilder,
) -> Result<(), EmitError> {
    for instr in instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(pred) = &m.predicate
            && let PredicateValueIR::Regex(string_id) = &pred.value
        {
            let pattern = strings.get_str(*string_id);
            regexes.intern(pattern, *string_id)?;
        }
    }
    Ok(())
}

fn emit_section(output: &mut Vec<u8>, data: &[u8]) {
    pad_to_section(output);
    output.extend_from_slice(data);
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
        bytes.extend_from_slice(&0u16.to_le_bytes()); // _pad is always 0
    }
    bytes
}
