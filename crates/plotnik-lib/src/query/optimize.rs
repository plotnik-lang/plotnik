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
use super::build_graph::{BuildGraph, BuildMatcher, NodeId};

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
        let (dead, _stats) = eliminate_epsilons(&mut self.graph);
        self.dead_nodes = dead;
    }
}

/// Run epsilon elimination on a BuildGraph.
///
/// Returns the set of dead node IDs that should be skipped during emission.
pub fn eliminate_epsilons(graph: &mut BuildGraph) -> (HashSet<NodeId>, OptimizeStats) {
    let mut stats = OptimizeStats::default();
    let mut dead_nodes: HashSet<NodeId> = HashSet::new();

    let predecessors = build_predecessor_map(graph);

    // Process nodes in reverse order to handle chains
    let node_count = graph.len() as NodeId;
    for id in (0..node_count).rev() {
        if dead_nodes.contains(&id) {
            continue;
        }

        let node = graph.node(id);
        if !is_eliminable_epsilon(node, graph) {
            if node.is_epsilon() {
                stats.epsilons_kept += 1;
            }
            continue;
        }

        let successor_id = node.successors[0];

        let successor = graph.node(successor_id);
        if !successor.ref_marker.is_none() && !node.effects.is_empty() {
            stats.epsilons_kept += 1;
            continue;
        }

        let effects_to_prepend = graph.node(id).effects.clone();
        let nav_to_transfer = graph.node(id).nav;
        let preds = predecessors.get(&id).cloned().unwrap_or_default();

        // Prepend effects to successor
        if !effects_to_prepend.is_empty() {
            let succ = graph.node_mut(successor_id);
            let mut new_effects = effects_to_prepend;
            new_effects.append(&mut succ.effects);
            succ.effects = new_effects;
        }

        // Transfer or merge nav
        let successor_nav = graph.node(successor_id).nav;
        if !nav_to_transfer.is_stay() {
            if successor_nav.is_stay() {
                graph.node_mut(successor_id).nav = nav_to_transfer;
            } else if can_merge_up(nav_to_transfer, successor_nav) {
                let merged = Nav::up(nav_to_transfer.level + successor_nav.level);
                graph.node_mut(successor_id).nav = merged;
            }
        }

        // Redirect predecessors to successor
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
        }

        redirect_definitions(graph, id, successor_id);

        dead_nodes.insert(id);
        stats.epsilons_eliminated += 1;
    }

    (dead_nodes, stats)
}

fn is_eliminable_epsilon(node: &super::build_graph::BuildNode, graph: &BuildGraph) -> bool {
    if !matches!(node.matcher, BuildMatcher::Epsilon) {
        return false;
    }

    if !node.ref_marker.is_none() {
        return false;
    }

    if node.successors.len() != 1 {
        return false;
    }

    let successor_id = node.successors[0];
    let successor = graph.node(successor_id);

    if !node.nav.is_stay() && !successor.nav.is_stay() {
        if !can_merge_up(node.nav, successor.nav) {
            return false;
        }
    }

    if !node.effects.is_empty() && !successor.ref_marker.is_none() {
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
