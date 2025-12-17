//! Epsilon elimination optimization pass.
//!
//! Reduces graph size by removing unnecessary epsilon transitions.
//!
//! # Safety Rules (from ADR-0005)
//!
//! An epsilon node CANNOT be eliminated if:
//! - It has a `RefMarker` (Enter/Exit)
//! - It has multiple successors (branch point)
//! - Its successor already has a `RefMarker`
//! - Both have non-Stay `Nav` that can't be merged

use std::collections::{HashMap, HashSet};

use crate::ir::{Nav, NavKind};

use super::Query;
use super::graph::{BuildGraph, BuildMatcher, NodeId};

/// Statistics from epsilon elimination.
#[derive(Debug, Default)]
pub struct OptimizeStats {
    pub epsilons_eliminated: usize,
    pub epsilons_kept: usize,
}

impl Query<'_> {
    /// Run epsilon elimination on the graph.
    ///
    /// Populates `dead_nodes` with eliminated node IDs.
    pub(super) fn optimize_graph(&mut self) {
        let (dead, _stats) = optimize_graph(&mut self.graph);
        self.dead_nodes = dead;
    }
}

/// Run epsilon elimination on a BuildGraph.
///
/// Returns the set of dead node IDs that should be skipped during emission.
pub fn optimize_graph(graph: &mut BuildGraph) -> (HashSet<NodeId>, OptimizeStats) {
    let mut stats = OptimizeStats::default();
    let mut dead_nodes: HashSet<NodeId> = HashSet::new();

    let mut predecessors = build_predecessor_map(graph);

    // Process nodes in reverse order to handle chains
    let node_count = graph.len() as NodeId;
    for id in (0..node_count).rev() {
        if dead_nodes.contains(&id) {
            continue;
        }

        // We need to clone specific fields or compute conditions before mutating to avoid borrow checker issues
        // if we were to hold a reference to 'node'.
        // Here we just inspect via indexing which is fine until we start mutating.

        // Check if eliminable
        if !is_eliminable_epsilon(id, graph, &predecessors) {
            let node = graph.node(id);
            if node.is_epsilon() {
                stats.epsilons_kept += 1;
            }
            continue;
        }

        let node_effects = graph.node(id).effects.clone();
        let node_nav = graph.node(id).nav;
        let successor_id = graph.node(id).successors[0];

        // 1. Prepend effects to successor
        if !node_effects.is_empty() {
            let succ = graph.node_mut(successor_id);
            let mut new_effects = node_effects;
            new_effects.append(&mut succ.effects);
            succ.effects = new_effects;
        }

        // 2. Transfer or merge nav
        let successor_nav = graph.node(successor_id).nav;
        if !node_nav.is_stay() {
            if successor_nav.is_stay() {
                graph.node_mut(successor_id).nav = node_nav;
            } else if can_merge_up(node_nav, successor_nav) {
                let merged = Nav::up(node_nav.level + successor_nav.level);
                graph.node_mut(successor_id).nav = merged;
            }
        }

        // 3. Redirect predecessors to successor
        let preds = predecessors.get(&id).cloned().unwrap_or_default();
        for pred_id in &preds {
            if dead_nodes.contains(pred_id) {
                continue;
            }
            let pred = graph.node_mut(*pred_id);
            for succ in &mut pred.successors {
                if *succ == id {
                    *succ = successor_id;
                }
            }
            // Update predecessor map: pred is now a predecessor of successor
            predecessors.entry(successor_id).or_default().push(*pred_id);
        }
        // Remove eliminated node from successor's predecessors
        if let Some(succ_preds) = predecessors.get_mut(&successor_id) {
            succ_preds.retain(|&p| p != id);
        }

        // 4. Update definitions that pointed to the eliminated node
        redirect_definitions(graph, id, successor_id);

        dead_nodes.insert(id);
        stats.epsilons_eliminated += 1;
    }

    (dead_nodes, stats)
}

fn is_eliminable_epsilon(
    id: NodeId,
    graph: &BuildGraph,
    predecessors: &HashMap<NodeId, Vec<NodeId>>,
) -> bool {
    let node = graph.node(id);

    if !matches!(node.matcher, BuildMatcher::Epsilon) {
        return false;
    }

    if node.ref_marker.is_some() {
        return false;
    }

    if node.successors.len() != 1 {
        return false;
    }

    let successor_id = node.successors[0];
    let successor = graph.node(successor_id);

    // Nav merge check
    if !node.nav.is_stay() && !successor.nav.is_stay() && !can_merge_up(node.nav, successor.nav) {
        return false;
    }

    // Don't eliminate if node has nav and successor is a join point.
    // Different paths may need different navigation.
    if !node.nav.is_stay() {
        let succ_pred_count = predecessors.get(&successor_id).map_or(0, |p| p.len());
        if succ_pred_count > 1 {
            return false;
        }
    }

    // Don't eliminate if node has effects and successor is a join point.
    if !node.effects.is_empty() {
        let succ_pred_count = predecessors.get(&successor_id).map_or(0, |p| p.len());
        if succ_pred_count > 1 {
            return false;
        }
    }

    // Don't eliminate if node has effects and successor has ref marker.
    // Effects must execute BEFORE ref marker (enter/exit), but merging moves them to successor
    // which effectively executes them "at" the successor.
    // If successor is Enter/Exit, the effects might conceptually belong to the edge before it.
    // Actually, effects on a node execute when traversing the edge TO that node.
    // If we merge A (effects) -> B (Enter), the effects of A are now on B.
    // So they execute when traversing TO B. This seems fine for Enter?
    // Wait, original logic said:
    if !node.effects.is_empty() && successor.ref_marker.is_some() {
        return false;
    }

    // Don't eliminate if epsilon has effects and successor has navigation.
    // Effects must execute BEFORE successor's nav.
    // If we merge, effects are on successor. When traversing to successor, effects run, then successor's nav runs.
    // This seems correct?
    // Original logic:
    // "Effects must execute BEFORE successor's nav/match, but prepending to effects list
    // would execute them AFTER nav/match." -> This comment in original code seems to imply effects run after nav?
    // In `graph.rs`, typical execution order is usually: Nav -> Match -> Effects (or similar).
    // If Nav happens first, then effects on the node happen.
    // If we merge A -> B. A has effects. B has Nav.
    // New B has A.effects + B.effects.
    // Execution: B.Nav -> B.Match -> A.effects -> B.effects.
    // But originally: A.Nav (Stay) -> A.Match (Epsilon) -> A.effects -> B.Nav -> ...
    // So A.effects happened BEFORE B.Nav.
    // Now A.effects happen AFTER B.Nav.
    // So if B.Nav is not Stay, we cannot merge if A has effects.
    if !node.effects.is_empty() && !successor.nav.is_stay() {
        return false;
    }

    true
}

fn build_predecessor_map(graph: &BuildGraph) -> HashMap<NodeId, Vec<NodeId>> {
    let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for (id, node) in graph.iter() {
        for &succ in &node.successors {
            predecessors.entry(succ).or_default().push(id);
        }
    }

    predecessors
}

fn can_merge_up(a: Nav, b: Nav) -> bool {
    a.kind == NavKind::Up && b.kind == NavKind::Up
}

fn redirect_definitions(graph: &mut BuildGraph, old_id: NodeId, new_id: NodeId) {
    let updates: Vec<_> = graph
        .definitions()
        .filter(|(_, entry)| *entry == old_id)
        .map(|(name, _)| name)
        .collect();

    for name in updates {
        graph.add_definition(name, new_id);
    }
}
