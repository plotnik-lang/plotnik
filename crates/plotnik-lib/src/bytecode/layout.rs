//! Cache-aligned instruction layout.
//!
//! Uses Pettis-Hansen inspired greedy chain extraction to place
//! hot paths contiguously and avoid cache line straddling.

use std::collections::{BTreeMap, HashSet};

use super::ids::StepId;
use super::ir::{Instruction, Label, LayoutResult};

const CACHE_LINE: usize = 64;
const STEP_SIZE: usize = 8;

/// Successor graph for layout analysis.
struct Graph {
    /// label -> list of successor labels
    successors: BTreeMap<Label, Vec<Label>>,
    /// label -> list of predecessor labels
    predecessors: BTreeMap<Label, Vec<Label>>,
}

impl Graph {
    fn build(instructions: &[Instruction]) -> Self {
        let mut successors: BTreeMap<Label, Vec<Label>> = BTreeMap::new();
        let mut predecessors: BTreeMap<Label, Vec<Label>> = BTreeMap::new();

        for instr in instructions {
            let label = instr.label();
            successors.entry(label).or_default();

            for succ in instr.successors() {
                if succ.is_accept() {
                    continue;
                }
                successors.entry(label).or_default().push(succ);
                predecessors.entry(succ).or_default().push(label);
            }
        }

        Self {
            successors,
            predecessors,
        }
    }

    fn successors(&self, label: Label) -> &[Label] {
        self.successors.get(&label).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn predecessor_count(&self, label: Label) -> usize {
        self.predecessors.get(&label).map(|v| v.len()).unwrap_or(0)
    }
}

/// Cache-aligned layout strategy.
pub struct CacheAligned;

impl CacheAligned {
    /// Compute layout for instructions with given entry points.
    ///
    /// Returns mapping from labels to step IDs and total step count.
    pub fn layout(instructions: &[Instruction], entries: &[Label]) -> LayoutResult {
        if instructions.is_empty() {
            return LayoutResult {
                label_to_step: BTreeMap::from([(Label::ACCEPT, StepId::ACCEPT)]),
                total_steps: 1,
            };
        }

        let graph = Graph::build(instructions);
        let label_to_instr: BTreeMap<Label, &Instruction> =
            instructions.iter().map(|i| (i.label(), i)).collect();

        let chains = extract_chains(&graph, instructions, entries);
        let ordered = order_chains(chains, entries);

        assign_step_ids(ordered, &label_to_instr)
    }
}

/// Extract linear chains from the control flow graph.
fn extract_chains(graph: &Graph, instructions: &[Instruction], entries: &[Label]) -> Vec<Vec<Label>> {
    let mut visited = HashSet::new();
    let mut chains = Vec::new();

    // Start with entry points (hot paths)
    for &entry in entries {
        if visited.contains(&entry) {
            continue;
        }
        chains.push(build_chain(entry, graph, &mut visited));
    }

    // Then remaining unvisited instructions
    for instr in instructions {
        let label = instr.label();
        if visited.contains(&label) {
            continue;
        }
        chains.push(build_chain(label, graph, &mut visited));
    }

    chains
}

/// Build a single chain starting from a label.
///
/// Extends the chain while there's a single unvisited successor with a single predecessor.
fn build_chain(start: Label, graph: &Graph, visited: &mut HashSet<Label>) -> Vec<Label> {
    let mut chain = vec![start];
    visited.insert(start);

    let mut current = start;
    while let [next] = graph.successors(current)
        && !visited.contains(next)
        && graph.predecessor_count(*next) == 1
    {
        chain.push(*next);
        visited.insert(*next);
        current = *next;
    }

    chain
}

/// Order chains: entries first, then by size (larger = hotter assumption).
fn order_chains(mut chains: Vec<Vec<Label>>, entries: &[Label]) -> Vec<Vec<Label>> {
    let entry_set: HashSet<Label> = entries.iter().copied().collect();

    // Partition into entry chains and non-entry chains
    let (mut entry_chains, mut other_chains): (Vec<_>, Vec<_>) = chains
        .drain(..)
        .partition(|chain| chain.first().map(|l| entry_set.contains(l)).unwrap_or(false));

    // Sort other chains by size (descending) for better locality
    other_chains.sort_by_key(|chain| std::cmp::Reverse(chain.len()));

    // Entry chains first, then others
    entry_chains.extend(other_chains);
    entry_chains
}

/// Assign step IDs with cache line awareness.
fn assign_step_ids(
    chains: Vec<Vec<Label>>,
    label_to_instr: &BTreeMap<Label, &Instruction>,
) -> LayoutResult {
    let mut mapping = BTreeMap::new();
    mapping.insert(Label::ACCEPT, StepId::ACCEPT);

    let mut current_step = 1u16; // 0 is ACCEPT
    let mut current_offset = 0usize; // Byte offset for cache alignment

    for chain in chains {
        for label in chain {
            let Some(instr) = label_to_instr.get(&label) else {
                continue;
            };
            let size = instr.size();

            // Cache line alignment for large instructions
            if size >= 48 {
                let line_offset = current_offset % CACHE_LINE;
                if line_offset + size > CACHE_LINE {
                    // Would straddle cache line - pad to next line
                    let padding_bytes = CACHE_LINE - line_offset;
                    let padding_steps = (padding_bytes / STEP_SIZE) as u16;
                    current_step += padding_steps;
                    current_offset += padding_bytes;
                }
            }

            mapping.insert(label, StepId(current_step));
            let step_count = (size / STEP_SIZE) as u16;
            current_step += step_count;
            current_offset += size;
        }
    }

    LayoutResult {
        label_to_step: mapping,
        total_steps: current_step,
    }
}
