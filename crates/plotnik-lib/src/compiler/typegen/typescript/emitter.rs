//! Core emitter struct and main emit logic.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::bytecode::{EntrypointsView, Module, StringsView, TypeId, TypesView};
use crate::core::Colors;

use super::Config;

pub struct Emitter<'a> {
    pub(super) types: TypesView<'a>,
    pub(super) strings: StringsView<'a>,
    pub(super) entrypoints: EntrypointsView<'a>,
    pub(super) config: Config,

    pub(super) type_names: HashMap<TypeId, String>,
    /// For collision avoidance when generating names.
    pub(super) used_names: BTreeSet<String>,
    pub(super) needs_node_type: bool,
    pub(super) emitted_types: HashSet<TypeId>,
    /// Cycle guard for `mark_node_type_usage`.
    pub(super) node_scan_seen: HashSet<TypeId>,
    pub(super) output: String,
}

struct EntrypointTypes {
    primary_names: HashMap<TypeId, String>,
    aliases: Vec<(String, TypeId)>,
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
            needs_node_type: false,
            emitted_types: HashSet::new(),
            node_scan_seen: HashSet::new(),
            output: String::new(),
        }
    }

    pub(super) fn colors(&self) -> Colors {
        self.config.colors
    }

    pub fn emit(mut self) -> String {
        self.assign_names();

        let entrypoint_types = self.entrypoint_types();
        let to_emit = self.entrypoint_emit_set();
        self.emit_named_types(&entrypoint_types.primary_names, to_emit);
        self.emit_remaining_entrypoints(&entrypoint_types.primary_names);
        self.emit_entrypoint_aliases(&entrypoint_types);
        self.finish_output()
    }

    fn entrypoint_types(&self) -> EntrypointTypes {
        let mut primary_names = HashMap::new();
        let mut aliases = Vec::new();
        for ep in self.entrypoints.iter() {
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
        EntrypointTypes {
            primary_names,
            aliases,
        }
    }

    fn entrypoint_emit_set(&self) -> HashSet<TypeId> {
        let mut to_emit = HashSet::new();
        for ep in self.entrypoints.iter() {
            self.collect_emit_set(ep.result_type(), &mut to_emit);
        }
        to_emit
    }

    fn emit_named_types(
        &mut self,
        primary_names: &HashMap<TypeId, String>,
        to_emit: HashSet<TypeId>,
    ) {
        for type_id in self.sort_topologically(to_emit) {
            if let Some(def_name) = primary_names.get(&type_id) {
                self.emit_type_definition(def_name, type_id);
            } else {
                self.emit_supporting_type(type_id);
            }
        }
    }

    fn emit_remaining_entrypoints(&mut self, primary_names: &HashMap<TypeId, String>) {
        // Emit remaining entrypoints (primitives, arrays, optionals)
        // These are not in to_emit because collect_emit_set skips them
        for (&type_id, name) in primary_names {
            if self.emitted_types.contains(&type_id) {
                continue;
            }
            self.emit_type_definition(name, type_id);
        }
    }

    fn emit_entrypoint_aliases(&mut self, entrypoint_types: &EntrypointTypes) {
        for (alias_name, type_id) in &entrypoint_types.aliases {
            if let Some(primary_name) = entrypoint_types.primary_names.get(type_id) {
                self.emit_type_alias(alias_name, primary_name);
            }
        }
    }

    fn finish_output(mut self) -> String {
        self.output.truncate(self.output.trim_end().len());
        self.output.push('\n');
        self.output
    }
}
