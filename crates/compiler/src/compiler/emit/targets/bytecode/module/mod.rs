//! Module-assembly emission phase: build the node-kind, grammar-field, and entrypoint wire
//! tables, serialize every section, and frame the module with its header and
//! checksum.

use crate::core::NodeKind;

use crate::bytecode::{
    Entrypoint, FieldEntry, HEADER_SIZE, Header, NodeKindEntry, SECTION_ALIGN, SPAN_NO_BINDING,
    SYMBOL_NAME_ENTRY_SIZE, SpanEntry,
};

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::{CaptureLayout, ResultSchema, ResultTypeLayout};
use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::{
    ConstantPool, EmitError, StringTableBuilder, TypeTableBuilder,
};
use crate::compiler::lower::ir::NfaGraph;
use crate::compiler::lower::spans::{SpanBindingIR, SpanTable};

use super::layout::compute_layout;
use super::string_table::seed_string_table;
use super::type_table::build_type_table;

/// The node-kind, grammar-field, and entrypoint wire tables. Built together because all
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
    type_layout: &'a ResultTypeLayout,
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
            type_layout: schema.type_layout(),
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

    /// Assemble the node-kind, grammar-field, and entrypoint tables, interning the last
    /// names into the string table. As the final string-table writer, this is where
    /// the string, type, and table capacities are sealed.
    pub(in crate::compiler::emit) fn build_tables(&mut self) -> Result<ModuleTables, EmitError> {
        let mut node_kinds: Vec<NodeKindEntry> = Vec::new();
        for (node_kind, node_id) in self.input.grammar.kind_entries() {
            let sym = match node_kind {
                NodeKind::Named(sym) | NodeKind::Anonymous(sym) => sym,
            };
            let name = self.strings.intern(sym, self.input.interner)?;
            node_kinds.push(NodeKindEntry::new(node_id, name));
        }

        let mut fields: Vec<FieldEntry> = Vec::new();
        for (sym, field_id) in self.input.grammar.field_entries() {
            let name = self.strings.intern(sym, self.input.interner)?;
            fields.push(FieldEntry::new(field_id, name));
        }

        let mut entrypoints: Vec<Entrypoint> = Vec::new();
        for (def_id, output) in self.input.iter_entrypoint_outputs() {
            let name_sym = self.input.definitions.definition(def_id).name();
            let name = self.strings.intern(name_sym, self.input.interner)?;
            let result_type =
                self.types
                    .resolve_output(output, self.input.type_analysis, self.type_layout)?;

            let entry = self
                .ir
                .entrypoints()
                .get(&def_id)
                .expect("entrypoint must have compiled target");
            let target = self
                .layout
                .code_addrs()
                .get(&entry.target)
                .copied()
                .expect("entrypoint target must have a code address");

            entrypoints.push(Entrypoint::new(name, target, result_type, entry.boundary));
        }

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
        instructions: &[u8],
    ) -> Result<Vec<u8>, EmitError> {
        let (str_blob, str_table) = pool.emit_strings()?;
        let (regex_blob, regex_table) = pool.emit_regexes()?;
        let (type_defs_bytes, type_members_bytes, type_names_bytes) = pool.emit_types();

        let node_kinds_bytes = emit_symbol_name_table(&tables.node_kinds, NodeKindEntry::to_bytes);
        let node_fields_bytes = emit_symbol_name_table(&tables.fields, FieldEntry::to_bytes);
        let entrypoints_bytes = emit_entrypoints(&tables.entrypoints);
        let spans_bytes = self.emit_spans()?;

        // Section order matches the bytecode layout:
        // Header → StringBlob → RegexBlob → StringTable → RegexTable →
        // NodeKinds → NodeFields → TypeDefs → TypeMembers → TypeNames →
        // Entrypoints → Instructions → Spans
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
        writer.emit_section(instructions);
        writer.emit_section(&spans_bytes);

        writer.finish_sections();
        let total_size =
            u32::try_from(writer.len()).map_err(|_| EmitError::ModuleTooLarge(writer.len()))?;
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
            entrypoints_count,
            instruction_word_count,
            spans_count,
            str_blob_size: u32::try_from(str_blob.len()).unwrap_or_else(|_| {
                panic!(
                    "string-table emission returned a {}-byte blob after validating it against the \
                     u32 bytecode limit",
                    str_blob.len()
                )
            }),
            regex_blob_size: u32::try_from(regex_blob.len()).unwrap_or_else(|_| {
                panic!(
                    "regex-table emission returned a {}-byte blob after validating it against the \
                     u32 bytecode limit",
                    regex_blob.len()
                )
            }),
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
                    let wire_type = self.types.resolve_type(
                        type_id,
                        self.input.type_analysis,
                        self.type_layout,
                    )?;
                    (u16::from(wire_type), SPAN_NO_BINDING)
                }
                Some(SpanBindingIR::Member(member)) => {
                    let descriptor = self.capture_layout.expect_member(member);
                    let wire_type = self
                        .types
                        .wire_id(self.type_layout.output_id(descriptor.parent_type))?;
                    (u16::from(wire_type), member.raw())
                }
                None => (SPAN_NO_BINDING, SPAN_NO_BINDING),
            };

            let source_id = u16::try_from(entry.source_id.0)
                .map_err(|_| EmitError::SourceIdTooLarge(entry.source_id.0))?;
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

fn emit_symbol_name_table<T: Copy>(
    symbols: &[T],
    encode: impl Fn(T) -> [u8; SYMBOL_NAME_ENTRY_SIZE],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(symbols.len() * SYMBOL_NAME_ENTRY_SIZE);
    for &sym in symbols {
        bytes.extend_from_slice(&encode(sym));
    }
    bytes
}

fn emit_entrypoints(entrypoints: &[Entrypoint]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entrypoints.len() * Entrypoint::SIZE);
    for ep in entrypoints {
        bytes.extend_from_slice(&u16::from(ep.name()).to_le_bytes());
        bytes.extend_from_slice(&ep.target().to_le_bytes());
        bytes.extend_from_slice(&u16::from(ep.result_type()).to_le_bytes());
        bytes.extend_from_slice(&ep.boundary().to_u16().to_le_bytes());
    }
    bytes
}
