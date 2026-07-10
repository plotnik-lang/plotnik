//! Core emitter struct and main emit logic.
//!
//! A pure renderer: every type name comes verbatim from the bytecode name
//! table, which the compiler's naming pass made complete (definition results,
//! path-derived composite names, `:: TypeName` annotations) and consistent
//! (one name never stands for two different shapes). Nothing is invented here;
//! a composite without a name — an enum variant payload, or foreign bytecode —
//! renders inline at its use sites.

use std::collections::{HashMap, HashSet};

use crate::bytecode::{
    EntrypointsView, Module, StringsView, TypeDef, TypeId, TypeMember, TypesView,
};
use crate::compiler::srcgen::sink::Sink;

use super::Config;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DtsRange {
    pub start: u32,
    pub end: u32,
    pub type_id: u16,
    pub member: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SemanticTag {
    pub(super) type_id: TypeId,
    pub(super) member: Option<u16>,
}

pub struct Emitter<'a> {
    pub(super) types: TypesView<'a>,
    pub(super) strings: StringsView<'a>,
    pub(super) entrypoints: EntrypointsView<'a>,
    pub(super) config: Config,

    /// Verbatim names from the bytecode name table.
    pub(super) type_names: HashMap<TypeId, String>,
    /// Names already declared. The same name may appear on several type ids
    /// only for structurally identical types (nominal twins from repeated
    /// annotations); one declaration serves them all.
    pub(super) declared_names: HashSet<String>,
    pub(super) needs_node_type: bool,
    pub(super) emitted_types: HashSet<TypeId>,
    /// Cycle guard for `mark_node_type_usage`.
    pub(super) node_scan_seen: HashSet<TypeId>,
    pub(super) sink: Sink<SemanticTag>,
}

impl<'a> Emitter<'a> {
    pub fn new(module: &'a Module, config: Config) -> Self {
        Self {
            types: module.types(),
            strings: module.strings(),
            entrypoints: module.entrypoints(),
            config,
            type_names: HashMap::new(),
            declared_names: HashSet::new(),
            needs_node_type: false,
            emitted_types: HashSet::new(),
            node_scan_seen: HashSet::new(),
            sink: Sink::new(),
        }
    }

    pub fn emit(mut self) -> String {
        self.emit_body();
        finish_output(self.sink.render(self.config.colors))
    }

    pub fn emit_mapped(mut self) -> (String, Vec<DtsRange>) {
        self.emit_body();
        let output = finish_output(self.sink.plain().to_string());
        let ranges = self
            .sink
            .tags()
            .iter()
            .map(|range| DtsRange {
                start: u32::try_from(range.start).expect("d.ts range start fits in u32"),
                end: u32::try_from(range.end).expect("d.ts range end fits in u32"),
                type_id: u16::from(range.tag.type_id),
                member: range.tag.member,
            })
            .collect();
        (output, ranges)
    }

    fn emit_body(&mut self) {
        self.load_names();
        self.mark_node_type_usage();
        if self.config.emit_node_interface && self.needs_node_type {
            self.emit_node_interface();
        }

        let mut to_emit = HashSet::new();
        for ep in self.entrypoints.iter() {
            self.collect_emit_set(ep.result_type(), &mut to_emit);
        }
        for type_id in self.sort_topologically(to_emit) {
            self.emit_declaration(type_id);
        }

        self.emit_undeclared_entrypoints();
    }

    fn load_names(&mut self) {
        for type_name in self.types.names() {
            let name = self.strings.get(type_name.name_id).to_string();
            self.type_names.entry(type_name.type_id).or_insert(name);
        }
    }

    /// Entrypoints whose result produced no named declaration: void queries
    /// (`export type Q = undefined;` — the query matches, but carries no data)
    /// and foreign bytecode with unnamed results (rendered inline).
    fn emit_undeclared_entrypoints(&mut self) {
        let remaining: Vec<(String, TypeId)> = self
            .entrypoints
            .iter()
            .map(|ep| (self.strings.get(ep.name()).to_string(), ep.result_type()))
            .filter(|(name, _)| !self.declared_names.contains(name))
            .collect();

        for (name, type_id) in remaining {
            let body = self.render_ty(type_id);
            self.declared_names.insert(name.clone());
            self.emit_type_decl(&name, type_id, body);
        }
    }

    pub(super) fn push_mapped(&mut self, text: &str, type_id: TypeId, member: Option<u16>) {
        self.sink.tagged(SemanticTag { type_id, member }, |sink| {
            sink.push(text);
        });
    }

    pub(super) fn members_of_with_indices(
        &self,
        def: &TypeDef,
    ) -> impl Iterator<Item = (u16, TypeMember)> + '_ {
        let (start, count) = def.member_range();
        (0..count).map(move |i| {
            let idx = start
                .checked_add(u16::from(i))
                .expect("type member index fits in u16");
            (idx, self.types.get_member(idx as usize))
        })
    }
}

fn finish_output(mut output: String) -> String {
    output.truncate(output.trim_end().len());
    output.push('\n');
    output
}
