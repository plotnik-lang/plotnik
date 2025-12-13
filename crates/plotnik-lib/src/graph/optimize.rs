//! Epsilon elimination optimization pass.
//!
//! Reduces graph size by removing unnecessary epsilon transitions.
//! This simplifies the graph for subsequent analysis passes and reduces
//! runtime traversal overhead.
//!
//! # Safety Rules (from ADR-0005)
//!
//! An epsilon node CANNOT be eliminated if:
//! - It has a `RefMarker` (Enter/Exit) — single slot constraint
//! - It has multiple successors (branch point)
//! - Its successor already has a `RefMarker` (would lose one)
//! - Both have non-Stay `Nav` that can't be merged (only unconstrained Up can merge)
//!
//! # Algorithm
//!
//! 1. Build predecessor map
//! 2. Identify eliminable epsilon nodes
//! 3. For each eliminable epsilon:
//!    - Prepend its effects to successor
//!    - Redirect all predecessors to successor
//!    - Mark epsilon as dead (will be skipped in emission)

use super::{BuildGraph, BuildMatcher, NodeId};
use crate::ir::{Nav, NavKind};
use std::collections::{HashMap, HashSet};

/// Statistics from epsilon elimination.
#[derive(Debug, Default)]
pub struct OptimizeStats {
    /// Number of epsilon nodes eliminated.
    pub epsilons_eliminated: usize,
    /// Number of epsilon nodes kept (branch points, ref markers, etc).
    pub epsilons_kept: usize,
}

/// Run epsilon elimination on the graph.
///
/// Returns the set of dead node IDs that should be skipped during emission.
pub fn eliminate_epsilons(graph: &mut BuildGraph) -> (HashSet<NodeId>, OptimizeStats) {
    let mut stats = OptimizeStats::default();
    let mut dead_nodes: HashSet<NodeId> = HashSet::new();

    // Build predecessor map: node -> list of predecessors
    let predecessors = build_predecessor_map(graph);

    // Process nodes in reverse order to handle chains
    // (eliminates inner epsilons before outer ones see them)
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

        // Get the single successor (already verified in is_eliminable_epsilon)
        let successor_id = node.successors[0];

        // Skip if successor has a RefMarker and we have effects
        // (can't merge effects into a ref transition)
        let successor = graph.node(successor_id);
        if !successor.ref_marker.is_none() && !node.effects.is_empty() {
            stats.epsilons_kept += 1;
            continue;
        }

        // Collect data needed for the merge
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
                // Simple transfer
                graph.node_mut(successor_id).nav = nav_to_transfer;
            } else if can_merge_up(nav_to_transfer, successor_nav) {
                // Merge unconstrained Up levels
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

        // Update definition entry points
        redirect_definitions(graph, id, successor_id);

        // Mark as dead
        dead_nodes.insert(id);
        stats.epsilons_eliminated += 1;
    }

    (dead_nodes, stats)
}

/// Check if an epsilon node can be eliminated.
fn is_eliminable_epsilon(node: &super::BuildNode, graph: &BuildGraph) -> bool {
    // Must be epsilon
    if !matches!(node.matcher, BuildMatcher::Epsilon) {
        return false;
    }

    // Must not have RefMarker
    if !node.ref_marker.is_none() {
        return false;
    }

    // Must have exactly one successor (not a branch point)
    if node.successors.len() != 1 {
        return false;
    }

    let successor_id = node.successors[0];
    let successor = graph.node(successor_id);

    // Can't merge if both have non-Stay nav, UNLESS both are unconstrained Up
    // (Up(n) + Up(m) = Up(n+m))
    if !node.nav.is_stay() && !successor.nav.is_stay() {
        if !can_merge_up(node.nav, successor.nav) {
            return false;
        }
    }

    // Can't merge if both have effects and successor has RefMarker
    // (effects must stay ordered relative to ref transitions)
    if !node.effects.is_empty() && !successor.ref_marker.is_none() {
        return false;
    }

    true
}

/// Build a map from each node to its predecessors.
fn build_predecessor_map(graph: &BuildGraph) -> HashMap<NodeId, Vec<NodeId>> {
    let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for (id, node) in graph.iter() {
        for &succ in &node.successors {
            predecessors.entry(succ).or_default().push(id);
        }
    }

    predecessors
}

/// Check if two Nav instructions can be merged (only unconstrained Up).
fn can_merge_up(a: Nav, b: Nav) -> bool {
    a.kind == NavKind::Up && b.kind == NavKind::Up
}

/// Update definition entry points if they pointed to eliminated node.
fn redirect_definitions(graph: &mut BuildGraph, old_id: NodeId, new_id: NodeId) {
    // Collect definitions that need updating
    let updates: Vec<_> = graph
        .definitions()
        .filter(|(_, entry)| *entry == old_id)
        .map(|(name, _)| name)
        .collect();

    // Apply updates
    for name in updates {
        graph.add_definition(name, new_id);
    }
}
