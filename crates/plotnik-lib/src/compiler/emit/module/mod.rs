#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Module-assembly emission phase: build the node-kind/field/entrypoint wire
//! tables, serialize every section, and frame the module with its header and
//! checksum.

use crate::core::NodeKind;

use crate::bytecode::{
    Entrypoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN, SymbolNameEntry,
};

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::emit::layout_map::LayoutMap;
use crate::compiler::emit::tables::{
    ConstantPool, EmitError, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::lower::ir::NfaGraph;

/// The node-kind, field, and entrypoint wire tables. Built together because all
/// three intern their names into the one string table.
pub struct ModuleTables {
    node_kinds: Vec<NodeKindEntry>,
    fields: Vec<FieldEntry>,
    entrypoints: Vec<Entrypoint>,
}

pub(in crate::compiler::emit) struct EmitPipeline<'a> {
    input: AnalysisArtifacts<'a>,
    ir: &'a NfaGraph,
    strings: StringTableBuilder,
    types: TypeTableBuilder,
    layout: LayoutMap,
}

impl<'a> EmitPipeline<'a> {
    pub(in crate::compiler::emit) fn new(
        input: AnalysisArtifacts<'a>,
        ir: &'a NfaGraph,
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
    pub(in crate::compiler::emit) fn build_tables(&mut self) -> Result<ModuleTables, EmitError> {
        let mut node_kinds: Vec<NodeKindEntry> = Vec::new();
        for (node_kind, node_id) in self.input.grammar.kind_entries() {
            let sym = match node_kind {
                NodeKind::Named(sym) | NodeKind::Anonymous(sym) => sym,
            };
            let name = self.strings.intern(sym, self.input.interner)?;
            node_kinds.push(NodeKindEntry::new(u16::from(node_id), name));
        }

        let mut fields: Vec<FieldEntry> = Vec::new();
        for (sym, field_id) in self.input.grammar.field_entries() {
            let name = self.strings.intern(sym, self.input.interner)?;
            fields.push(FieldEntry::new(u16::from(field_id), name));
        }

        let mut entrypoints: Vec<Entrypoint> = Vec::new();
        for (def_id, type_id) in self.input.type_analysis.iter_def_output() {
            let name_sym = self.input.dependency_analysis.def_name_sym(def_id);
            let name = self.strings.intern(name_sym, self.input.interner)?;
            let result_type = self.types.resolve_type(type_id, self.input.type_analysis)?;

            let target = self
                .ir
                .entrypoint_wrappers()
                .get(&def_id)
                .and_then(|label| self.layout.step_addrs().get(label))
                .copied()
                .expect("entrypoint must have compiled target");

            entrypoints.push(Entrypoint::new(name, target, result_type));
        }

        self.strings.validate()?;
        self.types.validate()?;
        if node_kinds.len() > EmitError::MAX_NODE_KINDS {
            return Err(EmitError::TooManyNodeKinds(node_kinds.len()));
        }
        if fields.len() > EmitError::MAX_NODE_FIELDS {
            return Err(EmitError::TooManyNodeFields(fields.len()));
        }
        if entrypoints.len() > EmitError::MAX_ENTRYPOINTS {
            return Err(EmitError::TooManyEntrypoints(entrypoints.len()));
        }

        Ok(ModuleTables {
            node_kinds,
            fields,
            entrypoints,
        })
    }

    /// Serialize every table into its section, then write the header and checksum.
    pub(in crate::compiler::emit) fn write_module(
        &self,
        pool: ConstantPool<'_>,
        tables: &ModuleTables,
        transitions: &[u8],
    ) -> Result<Vec<u8>, EmitError> {
        let (str_blob, str_table) = pool.emit_strings();
        let (regex_blob, regex_table) = pool.emit_regexes();
        let (type_defs_bytes, type_members_bytes, type_names_bytes) = pool.emit_types();

        let node_kinds_bytes = emit_symbol_name_table(&tables.node_kinds);
        let node_fields_bytes = emit_symbol_name_table(&tables.fields);
        let entrypoints_bytes = emit_entrypoints(&tables.entrypoints);

        // Section order matches the binary format:
        // Header → StringBlob → RegexBlob → StringTable → RegexTable →
        // NodeKinds → NodeFields → TypeDefs → TypeMembers → TypeNames →
        // Entrypoints → Transitions
        let mut writer = SectionWriter::new();

        writer.emit_section(&str_blob);
        writer.emit_section(&regex_blob);
        writer.emit_section(&str_table);
        writer.emit_section(&regex_table);
        writer.emit_section(&node_kinds_bytes);
        writer.emit_section(&node_fields_bytes);
        writer.emit_section(&type_defs_bytes);
        writer.emit_section(&type_members_bytes);
        writer.emit_section(&type_names_bytes);
        writer.emit_section(&entrypoints_bytes);
        writer.emit_section(transitions);

        writer.finish_sections();
        let total_size = writer.len() as u32;
        let str_table_count = checked_count(
            pool.string_count(),
            EmitError::MAX_STRINGS,
            EmitError::TooManyStrings,
        )?;
        let node_kinds_count = checked_count(
            tables.node_kinds.len(),
            EmitError::MAX_NODE_KINDS,
            EmitError::TooManyNodeKinds,
        )?;
        let node_fields_count = checked_count(
            tables.fields.len(),
            EmitError::MAX_NODE_FIELDS,
            EmitError::TooManyNodeFields,
        )?;
        let regex_table_count = checked_count(
            pool.regex_count(),
            EmitError::MAX_REGEXES,
            EmitError::TooManyRegexes,
        )?;
        let type_defs_count = checked_count(
            pool.type_defs_count(),
            EmitError::MAX_TYPES,
            EmitError::TooManyTypes,
        )?;
        let type_members_count = checked_count(
            pool.type_members_count(),
            EmitError::MAX_TYPE_MEMBERS,
            EmitError::TooManyTypeMembers,
        )?;
        let type_names_count = checked_count(
            pool.type_names_count(),
            EmitError::MAX_TYPE_NAMES,
            EmitError::TooManyTypeNames,
        )?;
        let entrypoints_count = checked_count(
            tables.entrypoints.len(),
            EmitError::MAX_ENTRYPOINTS,
            EmitError::TooManyEntrypoints,
        )?;
        let transitions_count = checked_count(
            self.layout.total_steps() as usize,
            EmitError::MAX_TRANSITIONS,
            EmitError::TooManyTransitions,
        )?;

        let mut header = Header {
            str_table_count,
            node_kinds_count,
            node_fields_count,
            regex_table_count,
            type_defs_count,
            type_members_count,
            type_names_count,
            entrypoints_count,
            transitions_count,
            str_blob_size: str_blob.len() as u32,
            regex_blob_size: regex_blob.len() as u32,
            total_size,
            ..Default::default()
        };
        header.checksum = crc32fast::hash(writer.body());
        writer.write_header(&header);

        Ok(writer.into_vec())
    }
}

fn checked_count(
    count: usize,
    max: usize,
    too_many: impl FnOnce(usize) -> EmitError,
) -> Result<u16, EmitError> {
    if count > max {
        return Err(too_many(count));
    }
    Ok(count as u16)
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

fn emit_symbol_name_table(symbols: &[SymbolNameEntry]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * SymbolNameEntry::SIZE);
    for sym in symbols {
        bytes.extend_from_slice(&sym.symbol.to_le_bytes());
        bytes.extend_from_slice(&u16::from(sym.name).to_le_bytes());
    }
    bytes
}

fn emit_entrypoints(entrypoints: &[Entrypoint]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entrypoints.len() * Entrypoint::SIZE);
    for ep in entrypoints {
        bytes.extend_from_slice(&u16::from(ep.name()).to_le_bytes());
        bytes.extend_from_slice(&ep.target().to_le_bytes());
        bytes.extend_from_slice(&u16::from(ep.result_type()).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // _pad is always 0
    }
    bytes
}
