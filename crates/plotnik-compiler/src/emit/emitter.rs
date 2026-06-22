//! Core bytecode emission logic.

use plotnik_compiler_core::GrammarBinding;
use plotnik_core::{Interner, NodeKind};

use crate::analyze::type_check::TypeContext;
use crate::bytecode::{CompileResult, Label};
use plotnik_bytecode::{
    Entrypoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN,
};

use super::EmitError;
use super::instructions::{emit_instructions, intern_predicate_strings, intern_regex_predicates};
use super::layout::CacheAligned;
use super::regex_table::RegexTableBuilder;
use super::string_table::StringTableBuilder;
use super::type_table::TypeTableBuilder;

#[derive(Clone, Copy)]
pub struct EmitInput<'a> {
    pub interner: &'a Interner,
    pub type_ctx: &'a TypeContext,
    pub grammar: &'a GrammarBinding,
}

/// Emit bytecode without the debug load self-check. Used by callers that load
/// the bytecode themselves (e.g. `check`'s dry run) and want a malformed-bytecode
/// case to surface as a diagnostic rather than the debug panic in [`emit`].
pub fn emit_unchecked(
    input: EmitInput<'_>,
    compile_result: &CompileResult,
) -> Result<Vec<u8>, EmitError> {
    let EmitInput {
        interner,
        type_ctx,
        grammar,
    } = input;

    // Every emitted effect's member ref names a type reachable from an entrypoint
    // result, so dead-type elimination roots at those results alone.
    let mut strings = StringTableBuilder::new();
    intern_predicate_strings(&compile_result.instructions, &mut strings);

    let mut types = TypeTableBuilder::new();
    types.build(type_ctx, interner, &mut strings)?;

    // Preamble entry FIRST ensures it gets the lowest address (step 0)
    let mut entry_labels: Vec<Label> = vec![compile_result.preamble_entry];
    entry_labels.extend(compile_result.def_entries.values().copied());
    let layout = CacheAligned::layout(&compile_result.instructions, &entry_labels);

    // Reject layouts whose step addresses overflow the u16 address space.
    // `total_steps` is computed in u32 precisely so this guard is reachable.
    if layout.total_steps() > u16::MAX as u32 {
        return Err(EmitError::TooManyTransitions(layout.total_steps() as usize));
    }

    let mut node_kinds: Vec<NodeKindEntry> = Vec::new();
    for (node_kind, node_id) in grammar.kind_entries() {
        let sym = match node_kind {
            NodeKind::Named(sym) | NodeKind::Anonymous(sym) => sym,
        };
        let name = strings.get_or_intern(sym, interner)?;
        node_kinds.push(NodeKindEntry::new(node_id.get(), name));
    }

    let mut fields: Vec<FieldEntry> = Vec::new();
    for (sym, field_id) in grammar.field_entries() {
        let name = strings.get_or_intern(sym, interner)?;
        fields.push(FieldEntry::new(field_id.get(), name));
    }

    let mut entrypoints: Vec<Entrypoint> = Vec::new();
    for (def_id, type_id) in type_ctx.iter_def_types() {
        let name_sym = type_ctx.def_name_sym(def_id);
        let name = strings.get_or_intern(name_sym, interner)?;
        let result_type = types.resolve_type(type_id, type_ctx)?;

        let target = compile_result
            .def_entries
            .get(&def_id)
            .and_then(|label| layout.step_addrs().get(label))
            .copied()
            .expect("entrypoint must have compiled target");

        entrypoints.push(Entrypoint::new(name, target, result_type));
    }

    strings.validate()?;
    types.validate()?;
    if node_kinds.len() > u16::MAX as usize {
        return Err(EmitError::TooManyNodeKinds(node_kinds.len()));
    }
    if fields.len() > u16::MAX as usize {
        return Err(EmitError::TooManyNodeFields(fields.len()));
    }
    if entrypoints.len() > 65535 {
        return Err(EmitError::TooManyEntrypoints(entrypoints.len()));
    }

    let mut regexes = RegexTableBuilder::new();
    intern_regex_predicates(&compile_result.instructions, &strings, &mut regexes)?;
    regexes.validate()?;

    let transitions_bytes = emit_instructions(
        &compile_result.instructions,
        &layout,
        &types,
        &strings,
        &regexes,
    )?;

    let (str_blob, str_table) = strings.emit();
    let (regex_blob, regex_table) = regexes.emit();
    let (type_defs_bytes, type_members_bytes, type_names_bytes) = types.emit();

    let node_types_bytes = emit_node_kinds(&node_kinds);
    let node_fields_bytes = emit_fields(&fields);
    let entrypoints_bytes = emit_entrypoints(&entrypoints);

    // Section order matches the binary format:
    // Header → StringBlob → RegexBlob → StringTable → RegexTable →
    // NodeTypes → NodeFields → TypeDefs → TypeMembers → TypeNames →
    // Entrypoints → Transitions
    let mut output = vec![0u8; HEADER_SIZE]; // Reserve header space

    emit_section(&mut output, &str_blob);
    emit_section(&mut output, &regex_blob);
    emit_section(&mut output, &str_table);
    emit_section(&mut output, &regex_table);
    emit_section(&mut output, &node_types_bytes);
    emit_section(&mut output, &node_fields_bytes);
    emit_section(&mut output, &type_defs_bytes);
    emit_section(&mut output, &type_members_bytes);
    emit_section(&mut output, &type_names_bytes);
    emit_section(&mut output, &entrypoints_bytes);
    emit_section(&mut output, &transitions_bytes);

    pad_to_section(&mut output);
    let total_size = output.len() as u32;

    let mut header = Header {
        str_table_count: strings.len() as u16,
        node_types_count: node_kinds.len() as u16,
        node_fields_count: fields.len() as u16,
        regex_table_count: regexes.len() as u16,
        type_defs_count: types.type_defs_count() as u16,
        type_members_count: types.type_members_count() as u16,
        type_names_count: types.type_names_count() as u16,
        entrypoints_count: entrypoints.len() as u16,
        transitions_count: layout.total_steps() as u16,
        str_blob_size: str_blob.len() as u32,
        regex_blob_size: regex_blob.len() as u32,
        total_size,
        ..Default::default()
    };
    header.checksum = crc32fast::hash(&output[HEADER_SIZE..]);
    output[..HEADER_SIZE].copy_from_slice(&header.to_bytes());

    Ok(output)
}

/// Emit bytecode, asserting in debug/test builds that the loader accepts it.
///
/// In debug/test builds this proves the emitter only ever produces bytecode the
/// loader accepts: every emission is gated through the full structural
/// validator. This makes "the compiler never emits invalid bytecode" an
/// enforced invariant across the whole test suite — and the load-time
/// checks (including the effect-stack verifier) the trust gate relies on
/// double as an emit-correctness oracle. Compiled out in release, where the
/// CLI's own `Module::load(...).expect(...)` is the boundary instead.
///
/// `check` deliberately bypasses this via [`emit_unchecked`]: it loads the
/// bytecode itself and reports a rejection as a diagnostic, so it must never
/// reach this panic, in debug or release.
pub fn emit(input: EmitInput<'_>, compile_result: &CompileResult) -> Result<Vec<u8>, EmitError> {
    let output = emit_unchecked(input, compile_result)?;
    #[cfg(debug_assertions)]
    if let Err(err) = plotnik_bytecode::Module::load(&output) {
        panic!("compiler emitted bytecode rejected by Module::load: {err:?}");
    }
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

fn emit_section(output: &mut Vec<u8>, data: &[u8]) {
    pad_to_section(output);
    output.extend_from_slice(data);
}

fn emit_node_kinds(symbols: &[NodeKindEntry]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * NodeKindEntry::SIZE);
    for sym in symbols {
        bytes.extend_from_slice(&sym.symbol.to_le_bytes());
        bytes.extend_from_slice(&sym.name.as_u16().to_le_bytes());
    }
    bytes
}

fn emit_fields(symbols: &[FieldEntry]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * FieldEntry::SIZE);
    for sym in symbols {
        bytes.extend_from_slice(&sym.symbol.to_le_bytes());
        bytes.extend_from_slice(&sym.name.as_u16().to_le_bytes());
    }
    bytes
}

fn emit_entrypoints(entrypoints: &[Entrypoint]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entrypoints.len() * Entrypoint::SIZE);
    for ep in entrypoints {
        bytes.extend_from_slice(&ep.name().as_u16().to_le_bytes());
        bytes.extend_from_slice(&ep.target().to_le_bytes());
        bytes.extend_from_slice(&ep.result_type().0.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // _pad is always 0
    }
    bytes
}
