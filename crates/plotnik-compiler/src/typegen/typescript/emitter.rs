//! Core emitter struct and main emit logic.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use plotnik_bytecode::{EntrypointsView, Module, StringsView, TypeId, TypesView};
use plotnik_core::Colors;

use super::Config;

/// TypeScript emitter from bytecode module.
pub struct Emitter<'a> {
    pub(super) types: TypesView<'a>,
    pub(super) strings: StringsView<'a>,
    pub(super) entrypoints: EntrypointsView<'a>,
    pub(super) config: Config,

    /// TypeId -> assigned name mapping
    pub(super) type_names: HashMap<TypeId, String>,
    /// Names already used (for collision avoidance)
    pub(super) used_names: BTreeSet<String>,
    /// Track which builtin types are referenced
    pub(super) node_referenced: bool,
    /// Track which types have been emitted
    pub(super) emitted: HashSet<TypeId>,
    /// Types visited during builtin reference collection (cycle detection)
    pub(super) refs_visited: HashSet<TypeId>,
    /// Output buffer
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
            node_referenced: false,
            emitted: HashSet::new(),
            refs_visited: HashSet::new(),
            output: String::new(),
        }
    }

    pub(super) fn c(&self) -> Colors {
        self.config.colors
    }

    /// Emit TypeScript for all entrypoint types.
    pub fn emit(mut self) -> String {
        self.prepare_emission();

        // Collect all entrypoints and their result types
        let mut primary_names: HashMap<TypeId, String> = HashMap::new();
        let mut aliases: Vec<(String, TypeId)> = Vec::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let name = self.strings.get(ep.name).to_string();
            let type_id = ep.result_type;

            match primary_names.entry(type_id) {
                Entry::Vacant(e) => {
                    e.insert(name);
                }
                Entry::Occupied(_) => {
                    aliases.push((name, type_id));
                }
            }
        }

        // Collect all reachable types starting from entrypoints
        let mut to_emit = HashSet::new();
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.collect_reachable_types(ep.result_type, &mut to_emit);
        }

        // Emit in topological order
        for type_id in self.sort_topologically(to_emit) {
            if let Some(def_name) = primary_names.get(&type_id) {
                self.emit_type_definition(def_name, type_id);
            } else {
                self.emit_generated_or_custom(type_id);
            }
        }

        // Emit remaining entrypoints (primitives, arrays, optionals)
        // These are not in to_emit because collect_reachable_types skips them
        for (&type_id, name) in &primary_names {
            if self.emitted.contains(&type_id) {
                continue;
            }
            self.emit_type_definition(name, type_id);
        }

        // Emit aliases
        for (alias_name, type_id) in aliases {
            if let Some(primary_name) = primary_names.get(&type_id) {
                self.emit_type_alias(&alias_name, primary_name);
            }
        }

        // Ensure exactly one trailing newline
        self.output.truncate(self.output.trim_end().len());
        self.output.push('\n');
        self.output
    }
}
