//! Dependency analysis for definitions.
//!
//! Computes the dependency graph of definitions and identifies Strongly Connected
//! Components (SCCs). The computed SCCs are exposed in reverse topological order
//! (leaves first), which is useful for passes that need to process dependencies
//! before dependents (like type inference).

use std::collections::HashMap;

use crate::core::Interner;
use indexmap::{IndexMap, IndexSet};

use crate::compiler::analyze::names::CollectedDefinitions;
use crate::compiler::diagnostics::Error;
use crate::compiler::ids::DefId;
use crate::compiler::limits::ReferenceLimits;
use crate::compiler::parse::ast::{DefRef, Pattern};

use super::definition_graph::{Definition, DefinitionGraph};

pub(in crate::compiler) fn build_definition_graph(
    collected: CollectedDefinitions,
    interner: &mut Interner,
    limits: ReferenceLimits,
) -> Result<DefinitionGraph, Error> {
    let (sccs, ids_by_symbol, mut outgoing, target_by_reference) = {
        // `strongconnect` recurses one frame per edge it follows. A chain like
        // `A = (B)`, `B = (C)`, … is flat source, so the parser cannot bound it;
        // graph construction owns the corresponding stack ceiling.
        let Some(topology) = TarjanScc::find(&collected, limits.max_depth) else {
            return Err(Error::RecursionLimitExceeded);
        };

        // Assign DefIds in SCC order (leaves first, so dependencies get lower IDs).
        let mut ids_by_symbol = HashMap::new();
        let mut definition_count = 0usize;
        let mut sccs = Vec::with_capacity(topology.sccs.len());
        for scc in &topology.sccs {
            let mut scc_ids = Vec::with_capacity(scc.len());
            for &name in scc {
                let symbol = interner.intern(name);
                let def_id = DefId::from_raw(
                    u32::try_from(definition_count).expect("definition count fits u32"),
                );
                ids_by_symbol.insert(symbol, def_id);
                definition_count += 1;
                scc_ids.push(def_id);
            }
            sccs.push(scc_ids);
        }

        let mut outgoing = vec![Vec::new(); definition_count];
        let mut target_by_reference = HashMap::new();
        for (&name, occurrences_by_target_name) in &topology.outgoing {
            let owner_symbol = interner
                .get(name)
                .expect("definition name must already be interned");
            let owner = ids_by_symbol
                .get(&owner_symbol)
                .copied()
                .expect("definition name must have a DefId");
            for (&target_name, occurrences) in occurrences_by_target_name {
                let symbol = interner
                    .get(target_name)
                    .expect("defined reference must already be interned");
                let target = ids_by_symbol
                    .get(&symbol)
                    .copied()
                    .expect("defined reference must have a DefId");
                outgoing[owner.index()].push(target);
                for reference in occurrences {
                    let previous = target_by_reference.insert(reference.clone(), target);
                    assert!(
                        previous.is_none(),
                        "an authored reference occurrence must have exactly one target"
                    );
                }
            }
        }

        (sccs, ids_by_symbol, outgoing, target_by_reference)
    };

    let definition_count = ids_by_symbol.len();
    let mut declaration_order = Vec::with_capacity(definition_count);
    let mut definitions = std::iter::repeat_with(|| None)
        .take(definition_count)
        .collect::<Vec<_>>();
    for (name, source, body) in collected.into_entries_in_declaration_order() {
        let symbol = interner
            .get(&name)
            .expect("collected definition name must already be interned");
        let def_id = *ids_by_symbol
            .get(&symbol)
            .expect("collected definition name must have a DefId");
        declaration_order.push(def_id);
        let outgoing_refs = std::mem::take(&mut outgoing[def_id.index()]);
        let previous = definitions[def_id.index()].replace(Definition::new(
            symbol,
            source,
            body,
            outgoing_refs,
        ));
        assert!(
            previous.is_none(),
            "a DefId must own exactly one definition"
        );
    }
    let definitions = definitions
        .into_iter()
        .map(|definition| definition.expect("every DefId must own a definition"))
        .collect();

    Ok(DefinitionGraph::new(
        sccs,
        ids_by_symbol,
        definitions,
        declaration_order,
        target_by_reference,
    ))
}

struct TarjanScc<'a> {
    definitions: &'a CollectedDefinitions,
    index: usize,
    stack: Vec<&'a str>,
    on_stack: IndexSet<&'a str>,
    indices: IndexMap<&'a str, usize>,
    lowlinks: IndexMap<&'a str, usize>,
    sccs: Vec<Vec<&'a str>>,
    outgoing: IndexMap<&'a str, ReferenceOccurrencesByTargetName<'a>>,
    /// Recursion ceiling and current depth: an acyclic reference chain deeper than
    /// this would overflow the native stack, so we stop and flag it instead.
    max_depth: u32,
    depth: u32,
    depth_exceeded: bool,
}

struct DependencyTopology<'a> {
    sccs: Vec<Vec<&'a str>>,
    outgoing: IndexMap<&'a str, ReferenceOccurrencesByTargetName<'a>>,
}

type ReferenceOccurrencesByTargetName<'a> = IndexMap<&'a str, Vec<DefRef>>;

impl<'a> TarjanScc<'a> {
    /// Returns the SCCs, or `None` if an acyclic reference chain ran past `max_depth`
    /// (the caller rejects the query rather than risk a stack overflow).
    fn find(
        definitions: &'a CollectedDefinitions,
        max_depth: u32,
    ) -> Option<DependencyTopology<'a>> {
        let mut finder = Self {
            definitions,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
            outgoing: IndexMap::new(),
            max_depth,
            depth: 0,
            depth_exceeded: false,
        };

        for name in definitions.names_in_declaration_order() {
            if !finder.indices.contains_key(name as &str) {
                finder.strongconnect(name);
                if finder.depth_exceeded {
                    return None;
                }
            }
        }

        Some(DependencyTopology {
            sccs: finder.sccs,
            outgoing: finder.outgoing,
        })
    }

    fn strongconnect(&mut self, name: &'a str) {
        // One frame per edge followed; stop before the chain outruns the stack. The
        // partial state left behind is discarded — `find` returns `None` and the query
        // is rejected — so we need not unwind it cleanly, only avoid recursing further.
        if self.depth >= self.max_depth {
            self.depth_exceeded = true;
            return;
        }
        self.depth += 1;

        self.indices.insert(name, self.index);
        self.lowlinks.insert(name, self.index);
        self.index += 1;
        self.stack.push(name);
        self.on_stack.insert(name);

        let body = self
            .definitions
            .body(name)
            .expect("collected definition name must have a body");
        let occurrences_by_target_name =
            collect_defined_reference_occurrences(body, self.definitions);
        for &target in occurrences_by_target_name.keys() {
            if !self.indices.contains_key(target) {
                self.strongconnect(target);
                if self.depth_exceeded {
                    self.depth -= 1;
                    return;
                }
                let reference_lowlink = self.lowlinks[target];
                let my_lowlink = self
                    .lowlinks
                    .get_mut(name)
                    .expect("lowlink for name was inserted at the start of strongconnect");
                *my_lowlink = (*my_lowlink).min(reference_lowlink);
            } else if self.on_stack.contains(target) {
                let reference_index = self.indices[target];
                let my_lowlink = self
                    .lowlinks
                    .get_mut(name)
                    .expect("lowlink for name was inserted at the start of strongconnect");
                *my_lowlink = (*my_lowlink).min(reference_index);
            }
        }
        self.outgoing.insert(name, occurrences_by_target_name);

        if self.lowlinks[name] == self.indices[name] {
            let mut scc = Vec::new();
            loop {
                let w = self
                    .stack
                    .pop()
                    .expect("SCC stack holds every on-stack node until its root pops");
                self.on_stack.swap_remove(&w);
                let done = w == name;
                scc.push(w);
                if done {
                    break;
                }
            }
            self.sccs.push(scc);
        }

        self.depth -= 1;
    }
}

/// Collect references that resolve within the transient definition map.
///
/// Returns only refs that point to defined names (filters out node kind references).
fn collect_defined_reference_occurrences<'a>(
    pattern: &Pattern,
    definitions: &'a CollectedDefinitions,
) -> ReferenceOccurrencesByTargetName<'a> {
    let mut occurrences_by_target_name = IndexMap::<_, Vec<_>>::new();
    for descendant in pattern.syntax().descendants() {
        let Some(r) = DefRef::cast(descendant) else {
            continue;
        };
        let Some(name_tok) = r.name() else { continue };
        let Some(key) = definitions.defined_name(name_tok.text()) else {
            continue;
        };
        occurrences_by_target_name.entry(key).or_default().push(r);
    }
    occurrences_by_target_name
}
