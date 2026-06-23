//! Cache-aligned instruction layout.
//!
//! Extracts linear chains from the control flow graph and places them
//! contiguously. Packs successor instructions into free space of predecessor
//! blocks for improved d-cache locality.

use std::collections::{BTreeMap, HashSet};

use crate::bytecode::{SECTION_ALIGN as CACHE_LINE, STEP_SIZE};

use crate::compiler::core::ir::{InstructionIR, Label, LayoutMap};

/// Intermediate representation for layout optimization.
struct BlockArena {
    blocks: Vec<Block>,
    label_to_block: BTreeMap<Label, usize>,
    label_to_offset: BTreeMap<Label, u8>,
}

/// A 64-byte cache-line block.
struct Block {
    placements: Vec<Placement>,
    used: u8,
}

/// An instruction placed within a block.
struct Placement {
    label: Label,
    offset: u8,
    size: u8,
}

impl Block {
    fn new() -> Self {
        Self {
            placements: Vec::new(),
            used: 0,
        }
    }

    fn free(&self) -> u8 {
        CACHE_LINE as u8 - self.used
    }

    fn can_fit(&self, size: u8) -> bool {
        self.free() >= size
    }

    fn place(&mut self, label: Label, size: u8) -> u8 {
        let offset = self.used;
        self.placements.push(Placement {
            label,
            offset,
            size,
        });
        self.used += size;
        offset
    }
}

impl BlockArena {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            label_to_block: BTreeMap::new(),
            label_to_offset: BTreeMap::new(),
        }
    }

    fn place(&mut self, label: Label, block_idx: usize, size: u8) {
        let offset = self.blocks[block_idx].place(label, size);
        self.label_to_block.insert(label, block_idx);
        self.label_to_offset.insert(label, offset);
    }

    fn move_to(&mut self, label: Label, new_block_idx: usize, size: u8) {
        if let Some(&old_block_idx) = self.label_to_block.get(&label)
            && let block = &mut self.blocks[old_block_idx]
            && let Some(pos) = block.placements.iter().position(|p| p.label == label)
        {
            let old_placement = block.placements.remove(pos);
            block.used -= old_placement.size;

            let mut offset = 0u8;
            for p in &mut block.placements {
                p.offset = offset;
                offset += p.size;
            }
        }

        let offset = self.blocks[new_block_idx].place(label, size);
        self.label_to_block.insert(label, new_block_idx);
        self.label_to_offset.insert(label, offset);
    }

    fn finalize(self) -> LayoutMap {
        let mut mapping = BTreeMap::new();
        // Accumulate in u32 so the step count never wraps; the emitter rejects a
        // layout exceeding the u16 address space before any `step as u16` (which
        // may wrap here) is read.
        let mut max_step_end: u32 = 0;

        for (block_idx, block) in self.blocks.iter().enumerate() {
            let block_base_step = (block_idx * CACHE_LINE / STEP_SIZE) as u32;
            for placement in &block.placements {
                let step = block_base_step + (placement.offset / STEP_SIZE as u8) as u32;
                mapping.insert(placement.label, step as u16);
                let step_end = step + (placement.size / STEP_SIZE as u8) as u32;
                max_step_end = max_step_end.max(step_end);
            }
        }

        LayoutMap::new(mapping, max_step_end)
    }
}

/// Block-to-block reference counts for scoring.
struct BlockEdges {
    /// (from_block, to_block) -> reference count
    direct: BTreeMap<(usize, usize), usize>,
    /// block -> list of predecessor blocks
    predecessors: BTreeMap<usize, Vec<usize>>,
}

impl BlockEdges {
    fn new() -> Self {
        Self {
            direct: BTreeMap::new(),
            predecessors: BTreeMap::new(),
        }
    }

    fn add_edge(&mut self, from_block: usize, to_block: usize) {
        *self.direct.entry((from_block, to_block)).or_default() += 1;
        let preds = self.predecessors.entry(to_block).or_default();
        if !preds.contains(&from_block) {
            preds.push(from_block);
        }
    }

    fn count(&self, from_block: usize, to_block: usize) -> usize {
        self.direct
            .get(&(from_block, to_block))
            .copied()
            .unwrap_or(0)
    }

    fn predecessors(&self, block: usize) -> &[usize] {
        self.predecessors
            .get(&block)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Score a candidate block for packing based on reference distance.
/// Direct refs count 1.0, 1-hop = 0.5, 2-hop = 0.25, capped at 3 hops.
fn block_score(target_block: usize, candidate_block: usize, refs: &BlockEdges) -> f32 {
    let mut score = 0.0f32;
    let mut frontier = vec![(candidate_block, 0u8)];
    let mut visited = HashSet::new();

    while let Some((block, dist)) = frontier.pop() {
        if !visited.insert(block) || dist > 3 {
            continue;
        }

        let direct_refs = refs.count(block, target_block);
        score += direct_refs as f32 / (1u32 << dist) as f32;

        for &pred in refs.predecessors(block) {
            frontier.push((pred, dist + 1));
        }
    }

    score
}

/// Successor graph for layout analysis.
struct Graph {
    /// label -> list of successor labels
    successors: BTreeMap<Label, Vec<Label>>,
    /// label -> list of predecessor labels
    predecessors: BTreeMap<Label, Vec<Label>>,
}

impl Graph {
    fn build(instructions: &[InstructionIR]) -> Self {
        let mut successors: BTreeMap<Label, Vec<Label>> = BTreeMap::new();
        let mut predecessors: BTreeMap<Label, Vec<Label>> = BTreeMap::new();

        for instr in instructions {
            let label = instr.label();
            successors.entry(label).or_default();

            for &succ in instr.successors() {
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
        self.successors
            .get(&label)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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
    pub fn layout(instructions: &[InstructionIR], entries: &[Label]) -> LayoutMap {
        if instructions.is_empty() {
            return LayoutMap::empty();
        }

        let graph = Graph::build(instructions);
        let label_to_instr: BTreeMap<Label, &InstructionIR> =
            instructions.iter().map(|i| (i.label(), i)).collect();

        let chains = extract_chains(&graph, instructions, entries);
        let ordered = order_chains(chains, entries);

        let mut layout = LayoutBuilder::new(&label_to_instr);
        layout.place_chains(&ordered);
        layout.pack_successors();
        layout.finish()
    }
}

/// Cache-aligned placement of instructions into blocks. Owns the `BlockArena`
/// under construction so the passes hand it off through `self` rather than
/// threading `ir`/`refs` between free functions.
struct LayoutBuilder<'a> {
    label_to_instr: &'a BTreeMap<Label, &'a InstructionIR>,
    ir: BlockArena,
}

impl<'a> LayoutBuilder<'a> {
    fn new(label_to_instr: &'a BTreeMap<Label, &'a InstructionIR>) -> Self {
        Self {
            label_to_instr,
            ir: BlockArena::new(),
        }
    }

    fn place_chains(&mut self, chains: &[Vec<Label>]) {
        for chain in chains {
            for &label in chain {
                let Some(instr) = self.label_to_instr.get(&label) else {
                    continue;
                };
                let size = instr.size() as u8;

                if self.ir.blocks.is_empty()
                    || !self
                        .ir
                        .blocks
                        .last()
                        .expect("blocks is non-empty by the guard above")
                        .can_fit(size)
                {
                    self.ir.blocks.push(Block::new());
                }
                let block_idx = self.ir.blocks.len() - 1;

                self.ir.place(label, block_idx, size);
            }
        }
    }

    fn block_refs(&self) -> BlockEdges {
        let mut refs = BlockEdges::new();

        for (&label, &block_idx) in &self.ir.label_to_block {
            let Some(instr) = self.label_to_instr.get(&label) else {
                continue;
            };
            for &succ in instr.successors() {
                if let Some(&succ_block) = self.ir.label_to_block.get(&succ)
                    && succ_block != block_idx
                {
                    refs.add_edge(block_idx, succ_block);
                }
            }
        }

        refs
    }

    /// Pack successor instructions into free space of predecessor blocks.
    ///
    /// When X → Y and X is in block B, try to move Y to an earlier block
    /// that has free space and high reference score to B.
    fn pack_successors(&mut self) {
        let refs = self.block_refs();

        let mut candidates: Vec<(Label, usize, usize)> = Vec::new();

        for (&label, &block_idx) in &self.ir.label_to_block {
            let Some(instr) = self.label_to_instr.get(&label) else {
                continue;
            };

            for &succ in instr.successors() {
                if let Some(&succ_block) = self.ir.label_to_block.get(&succ)
                    && succ_block > block_idx
                {
                    candidates.push((succ, succ_block, block_idx));
                }
            }
        }

        candidates.sort_by_key(|(_, succ_block, _)| std::cmp::Reverse(*succ_block));

        for (succ_label, _succ_block, pred_block) in candidates {
            let Some(&current_block) = self.ir.label_to_block.get(&succ_label) else {
                continue;
            };

            let Some(instr) = self.label_to_instr.get(&succ_label) else {
                continue;
            };
            let size = instr.size() as u8;

            // Prefer blocks that reference the predecessor block (cache locality)
            let scores: Vec<_> = (0..current_block)
                .map(|c| block_score(pred_block, c, &refs))
                .collect();
            let best = (0..current_block)
                .filter(|&c| self.ir.blocks[c].can_fit(size))
                .max_by(|&a, &b| {
                    scores[a]
                        .partial_cmp(&scores[b])
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

            if let Some(candidate) = best {
                self.ir.move_to(succ_label, candidate, size);
            }
        }
    }

    fn finish(self) -> LayoutMap {
        self.ir.finalize()
    }
}

fn extract_chains(
    graph: &Graph,
    instructions: &[InstructionIR],
    entries: &[Label],
) -> Vec<Vec<Label>> {
    let mut visited = HashSet::new();
    let mut chains = Vec::new();

    // Start with entry points (hot paths)
    for &entry in entries {
        if visited.contains(&entry) {
            continue;
        }
        chains.push(build_chain(entry, graph, &mut visited));
    }

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

    let (mut entry_chains, mut other_chains): (Vec<_>, Vec<_>) =
        chains.drain(..).partition(|chain| {
            chain
                .first()
                .map(|l| entry_set.contains(l))
                .unwrap_or(false)
        });

    // Sort other chains by size (descending) for better locality
    other_chains.sort_by_key(|chain| std::cmp::Reverse(chain.len()));

    entry_chains.extend(other_chains);
    entry_chains
}
