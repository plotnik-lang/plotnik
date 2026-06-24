#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Module-assembly emission phase: build the node-kind/field/entrypoint wire
//! tables, serialize every section, and frame the module with its header and
//! checksum.

use crate::core::NodeKind;

use crate::bytecode::{Entrypoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN};

use crate::compiler::lower::ir::{CompileResult, LayoutMap};
use crate::compiler::emit::tables::{
    EmitError, EmitInput, RegexTableBuilder, StringTableBuilder, TypeTableBuilder,
};

/// The node-kind, field, and entrypoint wire tables. Built together because all
/// three intern their names into the one string table.
pub struct WireTables {
    node_kinds: Vec<NodeKindEntry>,
    fields: Vec<FieldEntry>,
    entrypoints: Vec<Entrypoint>,
}

/// Assemble the node-kind, field, and entrypoint tables, interning the last
/// names into the string table. As the final string-table writer, this is where
/// the string, type, and table capacities are sealed.
pub fn build_tables(
    input: &EmitInput<'_>,
    ir: &CompileResult,
    types: &TypeTableBuilder,
    layout: &LayoutMap,
    mut strings: StringTableBuilder,
) -> Result<(WireTables, StringTableBuilder), EmitError> {
    let mut node_kinds: Vec<NodeKindEntry> = Vec::new();
    for (node_kind, node_id) in input.grammar.kind_entries() {
        let sym = match node_kind {
            NodeKind::Named(sym) | NodeKind::Anonymous(sym) => sym,
        };
        let name = strings.get_or_intern(sym, input.interner)?;
        node_kinds.push(NodeKindEntry::new(node_id.get(), name));
    }

    let mut fields: Vec<FieldEntry> = Vec::new();
    for (sym, field_id) in input.grammar.field_entries() {
        let name = strings.get_or_intern(sym, input.interner)?;
        fields.push(FieldEntry::new(field_id.get(), name));
    }

    let mut entrypoints: Vec<Entrypoint> = Vec::new();
    for (def_id, type_id) in input.type_ctx.iter_def_output() {
        let name_sym = input.dependency_analysis.def_name_sym(def_id);
        let name = strings.get_or_intern(name_sym, input.interner)?;
        let result_type = types.resolve_type(type_id, input.type_ctx)?;

        let target = ir
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

    Ok((
        WireTables {
            node_kinds,
            fields,
            entrypoints,
        },
        strings,
    ))
}

/// Serialize every table into its section, then write the header and checksum.
pub fn write_module(
    strings: &StringTableBuilder,
    types: &TypeTableBuilder,
    regexes: &RegexTableBuilder,
    layout: &LayoutMap,
    tables: &WireTables,
    transitions: &[u8],
) -> Vec<u8> {
    let (str_blob, str_table) = strings.emit();
    let (regex_blob, regex_table) = regexes.emit();
    let (type_defs_bytes, type_members_bytes, type_names_bytes) = types.emit();

    let node_types_bytes = emit_node_kinds(&tables.node_kinds);
    let node_fields_bytes = emit_fields(&tables.fields);
    let entrypoints_bytes = emit_entrypoints(&tables.entrypoints);

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
    emit_section(&mut output, transitions);

    pad_to_section(&mut output);
    let total_size = output.len() as u32;

    let mut header = Header {
        str_table_count: strings.len() as u16,
        node_types_count: tables.node_kinds.len() as u16,
        node_fields_count: tables.fields.len() as u16,
        regex_table_count: regexes.len() as u16,
        type_defs_count: types.type_defs_count() as u16,
        type_members_count: types.type_members_count() as u16,
        type_names_count: types.type_names_count() as u16,
        entrypoints_count: tables.entrypoints.len() as u16,
        transitions_count: layout.total_steps() as u16,
        str_blob_size: str_blob.len() as u32,
        regex_blob_size: regex_blob.len() as u32,
        total_size,
        ..Default::default()
    };
    header.checksum = crc32fast::hash(&output[HEADER_SIZE..]);
    output[..HEADER_SIZE].copy_from_slice(&header.to_bytes());

    output
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
