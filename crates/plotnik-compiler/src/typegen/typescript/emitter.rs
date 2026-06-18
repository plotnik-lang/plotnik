//! Core emitter struct and main emit logic.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use plotnik_bytecode::{EntrypointsView, Module, StringsView, TypeId, TypesView};
use plotnik_core::Colors;

use super::Config;

pub struct Emitter<'a> {
    pub(super) types: TypesView<'a>,
    pub(super) strings: StringsView<'a>,
    pub(super) entrypoints: EntrypointsView<'a>,
    pub(super) config: Config,

    pub(super) type_names: HashMap<TypeId, String>,
    /// For collision avoidance when generating names.
    pub(super) used_names: BTreeSet<String>,
    pub(super) node_reachable: bool,
    pub(super) emitted_types: HashSet<TypeId>,
    /// Cycle guard for `mark_node_reachable`.
    pub(super) node_scan_visited: HashSet<TypeId>,
    pub(super) output: String,
}

impl<'a> Emitter<'a> {
    pub fn new(module: &'a Module, config: Config) -> Self {
        Self {
            types: module.types(),
            strings: module.strings(),
            entrypoints: module.entrypoints(),
            config,
            type_names: HashMap::new(),
            used_names: BTreeSet::new(),
            node_reachable: false,
            emitted_types: HashSet::new(),
            node_scan_visited: HashSet::new(),
            output: String::new(),
        }
    }

    pub(super) fn colors(&self) -> Colors {
        self.config.colors
    }

    pub fn emit(mut self) -> String {
        self.assign_names();

        let mut primary_names: HashMap<TypeId, String> = HashMap::new();
        let mut aliases: Vec<(String, TypeId)> = Vec::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let name = self.strings.get(ep.name()).to_string();
            let type_id = ep.result_type();

            match primary_names.entry(type_id) {
                Entry::Vacant(e) => {
                    e.insert(name);
                }
                Entry::Occupied(_) => {
                    aliases.push((name, type_id));
                }
            }
        }

        let mut to_emit = HashSet::new();
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.collect_emit_set(ep.result_type(), &mut to_emit);
        }

        for type_id in self.sort_topologically(to_emit) {
            if let Some(def_name) = primary_names.get(&type_id) {
                self.emit_type_definition(def_name, type_id);
            } else {
                self.emit_supporting_type(type_id);
            }
        }

        // Emit remaining entrypoints (primitives, arrays, optionals)
        // These are not in to_emit because collect_emit_set skips them
        for (&type_id, name) in &primary_names {
            if self.emitted_types.contains(&type_id) {
                continue;
            }
            self.emit_type_definition(name, type_id);
        }

        for (alias_name, type_id) in aliases {
            if let Some(primary_name) = primary_names.get(&type_id) {
                self.emit_type_alias(&alias_name, primary_name);
            }
        }

        self.output.truncate(self.output.trim_end().len());
        self.output.push('\n');
        self.output
    }
}
