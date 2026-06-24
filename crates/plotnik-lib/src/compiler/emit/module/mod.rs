#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Module-assembly emission phase: build the node-kind/field/entrypoint wire
//! tables, serialize every section, and frame the module with its header and
//! checksum.

use crate::core::NodeKind;

use crate::bytecode::{Entrypoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN};

use crate::compiler::emit::tables::{
    ConstantPool, EmitError, EmitInput, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::lower::ir::{CompileResult, LayoutMap};

/// The node-kind, field, and entrypoint wire tables. Built together because all
/// three intern their names into the one string table.
pub struct WireTables {
    node_kinds: Vec<NodeKindEntry>,
    fields: Vec<FieldEntry>,
    entrypoints: Vec<Entrypoint>,
}

pub(in crate::compiler::emit) struct EmitPipeline<'a> {
    input: EmitInput<'a>,
    ir: &'a CompileResult,
    strings: StringTableBuilder,
    types: TypeTableBuilder,
    layout: LayoutMap,
}

impl<'a> EmitPipeline<'a> {
    pub(in crate::compiler::emit) fn new(
        input: EmitInput<'a>,
        ir: &'a CompileResult,
        strings: StringTableBuilder,
        types: TypeTableBuilder,
        layout: LayoutMap,
    ) -> Self {
        Self {
            input,
            ir,
            strings,
            types,
            layout,
        }
    }

    pub(in crate::compiler::emit) fn strings(&self) -> &StringTableBuilder {
        &self.strings
    }

    pub(in crate::compiler::emit) fn types(&self) -> &TypeTableBuilder {
        &self.types
    }

    pub(in crate::compiler::emit) fn layout(&self) -> &LayoutMap {
        &self.layout
    }

    /// Assemble the node-kind, field, and entrypoint tables, interning the last
    /// names into the string table. As the final string-table writer, this is where
    /// the string, type, and table capacities are sealed.
    pub(in crate::compiler::emit) fn build_tables(&mut self) -> Result<WireTables, EmitError> {
        let mut node_kinds: Vec<NodeKindEntry> = Vec::new();
        for (node_kind, node_id) in self.input.grammar.kind_entries() {
            let sym = match node_kind {
                NodeKind::Named(sym) | NodeKind::Anonymous(sym) => sym,
            };
            let name = self.strings.intern(sym, self.input.interner)?;
            node_kinds.push(NodeKindEntry::new(node_id.get(), name));
        }

        let mut fields: Vec<FieldEntry> = Vec::new();
        for (sym, field_id) in self.input.grammar.field_entries() {
            let name = self.strings.intern(sym, self.input.interner)?;
            fields.push(FieldEntry::new(field_id.get(), name));
        }

        let mut entrypoints: Vec<Entrypoint> = Vec::new();
        for (def_id, type_id) in self.input.type_ctx.iter_def_output() {
            let name_sym = self.input.dependency_analysis.def_name_sym(def_id);
            let name = self.strings.intern(name_sym, self.input.interner)?;
            let result_type = self.types.resolve_type(type_id, self.input.type_ctx)?;

            let target = self
                .ir
                .def_entries()
                .get(&def_id)
                .and_then(|label| self.layout.step_addrs().get(label))
                .copied()
                .expect("entrypoint must have compiled target");

            entrypoints.push(Entrypoint::new(name, target, result_type));
        }

        self.strings.validate()?;
        self.types.validate()?;
        if node_kinds.len() > u16::MAX as usize {
            return Err(EmitError::TooManyNodeKinds(node_kinds.len()));
        }
        if fields.len() > u16::MAX as usize {
            return Err(EmitError::TooManyNodeFields(fields.len()));
        }
        if entrypoints.len() > 65535 {
            return Err(EmitError::TooManyEntrypoints(entrypoints.len()));
        }

        Ok(WireTables {
            node_kinds,
            fields,
            entrypoints,
        })
    }

    /// Serialize every table into its section, then write the header and checksum.
    pub(in crate::compiler::emit) fn write_module(
        &self,
        pool: ConstantPool<'_>,
        tables: &WireTables,
        transitions: &[u8],
    ) -> Vec<u8> {
        let (str_blob, str_table) = pool.emit_strings();
        let (regex_blob, regex_table) = pool.emit_regexes();
        let (type_defs_bytes, type_members_bytes, type_names_bytes) = pool.emit_types();

        let node_types_bytes = emit_node_kinds(&tables.node_kinds);
        let node_fields_bytes = emit_fields(&tables.fields);
        let entrypoints_bytes = emit_entrypoints(&tables.entrypoints);

        // Section order matches the binary format:
        // Header → StringBlob → RegexBlob → StringTable → RegexTable →
        // NodeTypes → NodeFields → TypeDefs → TypeMembers → TypeNames →
        // Entrypoints → Transitions
        let mut writer = SectionWriter::new();

        writer.emit_section(&str_blob);
        writer.emit_section(&regex_blob);
        writer.emit_section(&str_table);
        writer.emit_section(&regex_table);
        writer.emit_section(&node_types_bytes);
        writer.emit_section(&node_fields_bytes);
        writer.emit_section(&type_defs_bytes);
        writer.emit_section(&type_members_bytes);
        writer.emit_section(&type_names_bytes);
        writer.emit_section(&entrypoints_bytes);
        writer.emit_section(transitions);

        writer.finish_sections();
        let total_size = writer.len() as u32;

        let mut header = Header {
            str_table_count: pool.string_count() as u16,
            node_types_count: tables.node_kinds.len() as u16,
            node_fields_count: tables.fields.len() as u16,
            regex_table_count: pool.regex_count() as u16,
            type_defs_count: pool.type_defs_count() as u16,
            type_members_count: pool.type_members_count() as u16,
            type_names_count: pool.type_names_count() as u16,
            entrypoints_count: tables.entrypoints.len() as u16,
            transitions_count: self.layout.total_steps() as u16,
            str_blob_size: str_blob.len() as u32,
            regex_blob_size: regex_blob.len() as u32,
            total_size,
            ..Default::default()
        };
        header.checksum = crc32fast::hash(writer.body());
        writer.write_header(&header);

        writer.into_vec()
    }
}

struct SectionWriter {
    output: Vec<u8>,
}

impl SectionWriter {
    fn new() -> Self {
        Self {
            output: vec![0u8; HEADER_SIZE],
        }
    }

    fn emit_section(&mut self, data: &[u8]) {
        self.pad_to_section();
        self.output.extend_from_slice(data);
    }

    fn finish_sections(&mut self) {
        self.pad_to_section();
    }

    fn len(&self) -> usize {
        self.output.len()
    }

    fn body(&self) -> &[u8] {
        &self.output[HEADER_SIZE..]
    }

    fn write_header(&mut self, header: &Header) {
        self.output[..HEADER_SIZE].copy_from_slice(&header.to_bytes());
    }

    fn into_vec(self) -> Vec<u8> {
        self.output
    }

    /// Pad a buffer to the section alignment boundary.
    fn pad_to_section(&mut self) {
        let rem = self.output.len() % SECTION_ALIGN;
        if rem == 0 {
            return;
        }

        let padding = SECTION_ALIGN - rem;
        self.output.resize(self.output.len() + padding, 0);
    }
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
