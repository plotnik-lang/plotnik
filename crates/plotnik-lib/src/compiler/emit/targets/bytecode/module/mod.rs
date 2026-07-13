//! Module-assembly emission phase: build the node-kind, grammar-field, and entry point wire
//! tables, serialize every section, and frame the module with its header and
//! checksum.

use crate::core::NodeKind;

use crate::bytecode::{
    EntryPoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN, SPAN_NO_BINDING,
    SpanEntry, SymbolNameEntry,
};

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::{CaptureLayout, ResultSchema};
use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::{
    ConstantPool, EmitError, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::lower::ir::NfaGraph;
use crate::compiler::lower::spans::{SpanBindingIR, SpanTable};

use super::layout::compute_layout;
use super::string_table::seed_string_table;
use super::type_table::build_type_table;

/// The node-kind, grammar-field, and entry point wire tables. Built together because all
/// three intern their names into the one string table.
pub struct ModuleTables {
    node_kinds: Vec<NodeKindEntry>,
    fields: Vec<FieldEntry>,
    entry_points: Vec<EntryPoint>,
}

pub(in crate::compiler::emit) struct EmitPipeline<'a> {
    input: AnalysisArtifacts<'a>,
    ir: &'a NfaGraph,
    strings: StringTableBuilder,
    types: TypeTableBuilder,
    layout: LayoutMap,
    capture_layout: &'a CaptureLayout,
}

impl<'a> EmitPipeline<'a> {
    /// Prepare every shared encoding table before module assembly begins.
    ///
    /// String seeding, type projection, and instruction layout are one ordered
    /// preparation phase: the resulting builders must agree before later table
    /// assembly interns the final symbol names. Keeping that sequence here
    /// avoids exposing six independently correlated constructor arguments.
    pub(in crate::compiler::emit) fn prepare(
        input: AnalysisArtifacts<'a>,
        ir: &'a NfaGraph,
        schema: &'a ResultSchema<'a>,
    ) -> Result<Self, EmitError> {
        let strings = seed_string_table(ir)?;
        let (types, strings) = build_type_table(schema, strings)?;
        let layout = compute_layout(ir)?;
        Ok(Self {
            input,
            ir,
            strings,
            types,
            layout,
            capture_layout: schema.layout(),
        })
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

    /// Assemble the node-kind, grammar-field, and entry point tables, interning the last
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

        let mut entry_points: Vec<EntryPoint> = Vec::new();
        for (def_id, output) in self.input.type_analysis.iter_entry_point_outputs() {
            let name_sym = self.input.dependency_analysis.def_name_sym(def_id);
            let name = self.strings.intern(name_sym, self.input.interner)?;
            let result_type = self
                .types
                .resolve_output(output, self.input.type_analysis)?;

            let target = self
                .ir
                .entry_point_wrappers()
                .get(&def_id)
                .and_then(|label| self.layout.code_addrs().get(label))
                .copied()
                .expect("entry point must have compiled target");

            entry_points.push(EntryPoint::new(name, target, result_type));
        }

        self.strings.validate()?;
        self.types.validate()?;
        if node_kinds.len() > EmitError::MAX_NODE_KINDS {
            return Err(EmitError::TooManyNodeKinds(node_kinds.len()));
        }
        if fields.len() > EmitError::MAX_NODE_FIELDS {
            return Err(EmitError::TooManyNodeFields(fields.len()));
        }
        if entry_points.len() > EmitError::MAX_ENTRY_POINTS {
            return Err(EmitError::TooManyEntryPoints(entry_points.len()));
        }

        Ok(ModuleTables {
            node_kinds,
            fields,
            entry_points,
        })
    }

    /// Serialize every table into its section, then write the header and checksum.
    pub(in crate::compiler::emit) fn write_module(
        &self,
        pool: ConstantPool<'_>,
        tables: &ModuleTables,
        instructions: &[u8],
    ) -> Result<Vec<u8>, EmitError> {
        let (str_blob, str_table) = pool.emit_strings();
        let (regex_blob, regex_table) = pool.emit_regexes();
        let (type_defs_bytes, type_members_bytes, type_names_bytes) = pool.emit_types();

        let node_kinds_bytes = emit_symbol_name_table(&tables.node_kinds);
        let node_fields_bytes = emit_symbol_name_table(&tables.fields);
        let entry_points_bytes = emit_entry_points(&tables.entry_points);
        let spans_bytes = self.emit_spans()?;

        // Section order matches the bytecode layout:
        // Header → StringBlob → RegexBlob → StringTable → RegexTable →
        // NodeKinds → NodeFields → TypeDefs → TypeMembers → TypeNames →
        // EntryPoints → Instructions → Spans
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
        writer.emit_section(&entry_points_bytes);
        writer.emit_section(instructions);
        writer.emit_section(&spans_bytes);

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
        let entry_points_count = checked_count(
            tables.entry_points.len(),
            EmitError::MAX_ENTRY_POINTS,
            EmitError::TooManyEntryPoints,
        )?;
        let instruction_word_count = checked_count(
            self.layout.total_words() as usize,
            EmitError::MAX_INSTRUCTION_WORDS,
            EmitError::TooManyInstructionWords,
        )?;
        let spans_count = checked_count(
            spans_count(self.ir.spans()),
            EmitError::MAX_SPANS,
            EmitError::TooManySpans,
        )?;

        let mut header = Header {
            str_table_count,
            node_kinds_count,
            node_fields_count,
            regex_table_count,
            type_defs_count,
            type_members_count,
            type_names_count,
            entry_points_count,
            instruction_word_count,
            spans_count,
            str_blob_size: str_blob.len() as u32,
            regex_blob_size: regex_blob.len() as u32,
            total_size,
            ..Default::default()
        };
        header.checksum = crc32fast::hash(writer.body());
        writer.write_header(&header);

        Ok(writer.into_vec())
    }

    fn emit_spans(&self) -> Result<Vec<u8>, EmitError> {
        let Some(spans) = self.ir.spans() else {
            return Ok(Vec::new());
        };

        let mut bytes = Vec::with_capacity(spans.entries.len() * SpanEntry::SIZE);
        for entry in &spans.entries {
            let (type_id, member) = match entry.binding {
                Some(SpanBindingIR::Type(type_id)) => {
                    let wire_type = self.types.resolve_type(type_id, self.input.type_analysis)?;
                    (u16::from(wire_type), SPAN_NO_BINDING)
                }
                Some(SpanBindingIR::Member(member_ref)) => {
                    let wire_type = self
                        .types
                        .lookup(member_ref.parent_type)
                        .expect("validated span member binding must reference an emitted type");
                    let member = self
                        .capture_layout
                        .scope(member_ref.parent_type)
                        .expect("validated span member binding must reference a capture scope")
                        .absolute_index(member_ref.relative_index);
                    (u16::from(wire_type), member)
                }
                None => (SPAN_NO_BINDING, SPAN_NO_BINDING),
            };

            let source_id =
                u16::try_from(entry.source_id.0).expect("source id must fit in span entry");
            let span = SpanEntry {
                source_id,
                kind: entry.kind,
                start: u32::from(entry.range.start()),
                end: u32::from(entry.range.end()),
                type_id,
                member,
            };
            bytes.extend_from_slice(&span.to_bytes());
        }

        Ok(bytes)
    }
}

fn spans_count(spans: Option<&SpanTable>) -> usize {
    spans.map_or(0, |spans| spans.entries.len())
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

fn emit_entry_points(entry_points: &[EntryPoint]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entry_points.len() * EntryPoint::SIZE);
    for ep in entry_points {
        bytes.extend_from_slice(&u16::from(ep.name()).to_le_bytes());
        bytes.extend_from_slice(&ep.target().to_le_bytes());
        bytes.extend_from_slice(&u16::from(ep.result_type()).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // _pad is always 0
    }
    bytes
}
