//! Analysis pass for emission preparation.
//!
//! This module prepares a `BuildGraph` for emission to the binary format
//! by computing counts, interning strings, and mapping node IDs.
//!
//! # Three-Phase Construction (ADR-0004)
//!
//! 1. **Analysis** (this module): Count elements, intern strings
//! 2. **Layout**: Compute aligned offsets, allocate once
//! 3. **Emission**: Write to buffer
//!
//! # String Interning
//!
//! All strings (field names, variant tags, node kinds, definition names)
//! are deduplicated. Identical strings share storage and `StringId`.

use super::{BuildEffect, BuildGraph, BuildMatcher, NodeId};
use crate::ir::StringId;
use indexmap::IndexMap;
use std::collections::HashSet;

/// Result of analyzing a BuildGraph for emission.
#[derive(Debug)]
pub struct AnalysisResult<'src> {
    /// String interner with all unique strings.
    pub strings: StringInterner<'src>,

    /// Mapping from BuildGraph NodeId to emission index.
    /// Dead nodes map to `None`.
    pub node_map: Vec<Option<u32>>,

    /// Number of live transitions to emit.
    pub transition_count: u32,

    /// Total successor slots needed in the spill segment.
    /// (Only for nodes with >8 successors)
    pub spilled_successor_count: u32,

    /// Total effects across all nodes.
    pub effect_count: u32,

    /// Total negated fields across all matchers.
    pub negated_field_count: u32,

    /// Number of definition entrypoints.
    pub entrypoint_count: u32,
}

/// String interner for deduplication.
///
/// Strings are stored in insertion order. `StringId` is the index.
#[derive(Debug, Default)]
pub struct StringInterner<'src> {
    /// Map from string content to its ID.
    index: IndexMap<&'src str, StringId>,
}

impl<'src> StringInterner<'src> {
    pub fn new() -> Self {
        Self {
            index: IndexMap::new(),
        }
    }

    /// Intern a string, returning its ID.
    /// Returns existing ID if already interned.
    pub fn intern(&mut self, s: &'src str) -> StringId {
        let next_id = self.index.len() as StringId;
        *self.index.entry(s).or_insert(next_id)
    }

    /// Get the ID of an already-interned string.
    pub fn get(&self, s: &str) -> Option<StringId> {
        self.index.get(s).copied()
    }

    /// Iterate over all strings in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&'src str, StringId)> + '_ {
        self.index.iter().map(|(s, id)| (*s, *id))
    }

    /// Number of interned strings.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns true if no strings have been interned.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Total byte length of all strings.
    pub fn total_bytes(&self) -> usize {
        self.index.keys().map(|s| s.len()).sum()
    }
}

/// Analyze a BuildGraph for emission.
///
/// The `dead_nodes` set contains nodes eliminated by optimization passes.
/// These are skipped during analysis and won't appear in the output.
pub fn analyze<'src>(
    graph: &BuildGraph<'src>,
    dead_nodes: &HashSet<NodeId>,
) -> AnalysisResult<'src> {
    let mut strings = StringInterner::new();
    let mut node_map: Vec<Option<u32>> = vec![None; graph.len()];

    let mut transition_count: u32 = 0;
    let mut spilled_successor_count: u32 = 0;
    let mut effect_count: u32 = 0;
    let mut negated_field_count: u32 = 0;

    // First pass: map live nodes to emission indices and count elements
    for (id, node) in graph.iter() {
        if dead_nodes.contains(&id) {
            continue;
        }

        node_map[id as usize] = Some(transition_count);
        transition_count += 1;

        // Count successors that spill (>8)
        let live_successors = count_live_successors(node, dead_nodes);
        if live_successors > 8 {
            spilled_successor_count += live_successors as u32;
        }

        // Count effects
        effect_count += node.effects.len() as u32;

        // Intern strings and count negated fields from matcher
        match &node.matcher {
            BuildMatcher::Node {
                kind,
                field,
                negated_fields,
            } => {
                strings.intern(kind);
                if let Some(f) = field {
                    strings.intern(f);
                }
                for nf in negated_fields {
                    strings.intern(nf);
                }
                negated_field_count += negated_fields.len() as u32;
            }
            BuildMatcher::Anonymous { literal, field } => {
                strings.intern(literal);
                if let Some(f) = field {
                    strings.intern(f);
                }
            }
            BuildMatcher::Wildcard { field } => {
                if let Some(f) = field {
                    strings.intern(f);
                }
            }
            BuildMatcher::Epsilon => {}
        }

        // Intern strings from effects
        for effect in &node.effects {
            match effect {
                BuildEffect::Field(name) => {
                    strings.intern(name);
                }
                BuildEffect::StartVariant(tag) => {
                    strings.intern(tag);
                }
                _ => {}
            }
        }

        // Intern ref name if present
        if let Some(name) = node.ref_name {
            strings.intern(name);
        }
    }

    // Intern definition names
    let entrypoint_count = graph.definitions().count() as u32;
    for (name, _) in graph.definitions() {
        strings.intern(name);
    }

    AnalysisResult {
        strings,
        node_map,
        transition_count,
        spilled_successor_count,
        effect_count,
        negated_field_count,
        entrypoint_count,
    }
}

/// Count live successors (excluding dead nodes).
fn count_live_successors(node: &super::BuildNode, dead_nodes: &HashSet<NodeId>) -> usize {
    node.successors
        .iter()
        .filter(|s| !dead_nodes.contains(s))
        .count()
}
