//! Tests for epsilon elimination optimization pass.

use std::collections::HashSet;

use super::*;
use crate::graph::{BuildEffect, BuildMatcher, NodeId, RefMarker};

fn dump_graph(graph: &BuildGraph) -> String {
    let mut out = String::new();

    for (name, entry) in graph.definitions() {
        out.push_str(&format!("{} = N{}\n", name, entry));
    }
    if graph.definitions().next().is_some() {
        out.push('\n');
    }

    for (id, node) in graph.iter() {
        out.push_str(&format!("N{}: ", id));

        match &node.matcher {
            BuildMatcher::Epsilon => out.push('ε'),
            BuildMatcher::Node {
                kind,
                field,
                negated_fields,
            } => {
                out.push_str(&format!("({})", kind));
                if let Some(f) = field {
                    out.push_str(&format!(" @{}", f));
                }
                for neg in negated_fields {
                    out.push_str(&format!(" !{}", neg));
                }
            }
            BuildMatcher::Anonymous { literal, field } => {
                out.push_str(&format!("\"{}\"", literal));
                if let Some(f) = field {
                    out.push_str(&format!(" @{}", f));
                }
            }
            BuildMatcher::Wildcard { field } => {
                out.push('_');
                if let Some(f) = field {
                    out.push_str(&format!(" @{}", f));
                }
            }
        }

        match &node.ref_marker {
            RefMarker::None => {}
            RefMarker::Enter { ref_id } => out.push_str(&format!(" +Enter({})", ref_id)),
            RefMarker::Exit { ref_id } => out.push_str(&format!(" +Exit({})", ref_id)),
        }

        for effect in &node.effects {
            let eff = match effect {
                BuildEffect::CaptureNode => "Capture".to_string(),
                BuildEffect::StartArray => "StartArray".to_string(),
                BuildEffect::PushElement => "Push".to_string(),
                BuildEffect::EndArray => "EndArray".to_string(),
                BuildEffect::StartObject => "StartObj".to_string(),
                BuildEffect::EndObject => "EndObj".to_string(),
                BuildEffect::Field(f) => format!("Field({})", f),
                BuildEffect::StartVariant(v) => format!("Variant({})", v),
                BuildEffect::EndVariant => "EndVariant".to_string(),
                BuildEffect::ToString => "ToString".to_string(),
            };
            out.push_str(&format!(" [{}]", eff));
        }

        if node.successors.is_empty() {
            out.push_str(" → ∅");
        } else {
            out.push_str(" → ");
            let succs: Vec<_> = node.successors.iter().map(|s| format!("N{}", s)).collect();
            out.push_str(&succs.join(", "));
        }

        out.push('\n');
    }

    out
}

fn dump_live_graph(graph: &BuildGraph, dead: &HashSet<NodeId>) -> String {
    let mut out = String::new();

    for (name, entry) in graph.definitions() {
        out.push_str(&format!("{} = N{}\n", name, entry));
    }
    if graph.definitions().next().is_some() {
        out.push('\n');
    }

    for (id, node) in graph.iter() {
        if dead.contains(&id) {
            continue;
        }

        out.push_str(&format!("N{}: ", id));

        match &node.matcher {
            BuildMatcher::Epsilon => out.push('ε'),
            BuildMatcher::Node { kind, .. } => out.push_str(&format!("({})", kind)),
            BuildMatcher::Anonymous { literal, .. } => out.push_str(&format!("\"{}\"", literal)),
            BuildMatcher::Wildcard { .. } => out.push('_'),
        }

        match &node.ref_marker {
            RefMarker::None => {}
            RefMarker::Enter { ref_id } => out.push_str(&format!(" +Enter({})", ref_id)),
            RefMarker::Exit { ref_id } => out.push_str(&format!(" +Exit({})", ref_id)),
        }

        for effect in &node.effects {
            let eff = match effect {
                BuildEffect::CaptureNode => "Capture".to_string(),
                BuildEffect::StartArray => "StartArray".to_string(),
                BuildEffect::PushElement => "Push".to_string(),
                BuildEffect::EndArray => "EndArray".to_string(),
                BuildEffect::StartObject => "StartObj".to_string(),
                BuildEffect::EndObject => "EndObj".to_string(),
                BuildEffect::Field(f) => format!("Field({})", f),
                BuildEffect::StartVariant(v) => format!("Variant({})", v),
                BuildEffect::EndVariant => "EndVariant".to_string(),
                BuildEffect::ToString => "ToString".to_string(),
            };
            out.push_str(&format!(" [{}]", eff));
        }

        if node.successors.is_empty() {
            out.push_str(" → ∅");
        } else {
            out.push_str(" → ");
            let succs: Vec<_> = node
                .successors
                .iter()
                .filter(|s| !dead.contains(s))
                .map(|s| format!("N{}", s))
                .collect();
            out.push_str(&succs.join(", "));
        }

        out.push('\n');
    }

    out
}

#[test]
fn eliminates_simple_epsilon_chain() {
    let mut g = BuildGraph::new();

    // Build: ε → ε → (identifier)
    let id = g.add_matcher(BuildMatcher::node("identifier"));
    let e1 = g.add_epsilon();
    let e2 = g.add_epsilon();
    g.connect(e2, e1);
    g.connect(e1, id);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (identifier) → ∅
    N1: ε → N0
    N2: ε → N1
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 2);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (identifier) → ∅
    "#);
}

#[test]
fn keeps_branch_point_epsilon() {
    let mut g = BuildGraph::new();

    // Build alternation: ε → [A, B]
    let a = g.add_matcher(BuildMatcher::node("a"));
    let b = g.add_matcher(BuildMatcher::node("b"));
    let branch = g.add_epsilon();
    g.connect(branch, a);
    g.connect(branch, b);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (a) → ∅
    N1: (b) → ∅
    N2: ε → N0, N1
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 0);
    assert_eq!(stats.epsilons_kept, 1);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (a) → ∅
    N1: (b) → ∅
    N2: ε → N0, N1
    "#);
}

#[test]
fn keeps_epsilon_with_enter_marker() {
    let mut g = BuildGraph::new();

    let target = g.add_matcher(BuildMatcher::node("target"));
    let enter = g.add_epsilon();
    g.node_mut(enter).set_ref_marker(RefMarker::enter(0));
    g.connect(enter, target);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (target) → ∅
    N1: ε +Enter(0) → N0
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 0);
    assert_eq!(stats.epsilons_kept, 1);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (target) → ∅
    N1: ε +Enter(0) → N0
    "#);
}

#[test]
fn keeps_epsilon_with_exit_marker() {
    let mut g = BuildGraph::new();

    let target = g.add_matcher(BuildMatcher::node("target"));
    let exit = g.add_epsilon();
    g.node_mut(exit).set_ref_marker(RefMarker::exit(0));
    g.connect(exit, target);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 0);
    assert_eq!(stats.epsilons_kept, 1);
    assert!(dead.is_empty());
}

#[test]
fn merges_effects_into_successor() {
    let mut g = BuildGraph::new();

    // ε[StartArray] → ε[EndArray] → (identifier)[Capture]
    let id = g.add_matcher(BuildMatcher::node("identifier"));
    g.node_mut(id).add_effect(BuildEffect::CaptureNode);

    let end_arr = g.add_epsilon();
    g.node_mut(end_arr).add_effect(BuildEffect::EndArray);
    g.connect(end_arr, id);

    let start_arr = g.add_epsilon();
    g.node_mut(start_arr).add_effect(BuildEffect::StartArray);
    g.connect(start_arr, end_arr);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (identifier) [Capture] → ∅
    N1: ε [EndArray] → N0
    N2: ε [StartArray] → N1
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 2);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (identifier) [StartArray] [EndArray] [Capture] → ∅
    "#);
}

#[test]
fn redirects_multiple_predecessors() {
    let mut g = BuildGraph::new();

    // A → ε → C
    // B ↗
    let c = g.add_matcher(BuildMatcher::node("c"));
    let eps = g.add_epsilon();
    let a = g.add_matcher(BuildMatcher::node("a"));
    let b = g.add_matcher(BuildMatcher::node("b"));

    g.connect(eps, c);
    g.connect(a, eps);
    g.connect(b, eps);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (c) → ∅
    N1: ε → N0
    N2: (a) → N1
    N3: (b) → N1
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 1);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (c) → ∅
    N2: (a) → N0
    N3: (b) → N0
    "#);
}

#[test]
fn updates_definition_entry_point() {
    let mut g = BuildGraph::new();

    // Def = ε → (identifier)
    let id = g.add_matcher(BuildMatcher::node("identifier"));
    let eps = g.add_epsilon();
    g.connect(eps, id);
    g.add_definition("Def", eps);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    Def = N1

    N0: (identifier) → ∅
    N1: ε → N0
    "#);

    let (dead, _stats) = eliminate_epsilons(&mut g);

    // Definition should now point to identifier node
    assert_eq!(g.definition("Def"), Some(0));
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    Def = N0

    N0: (identifier) → ∅
    "#);
}

#[test]
fn keeps_exit_epsilon_with_no_successor() {
    let mut g = BuildGraph::new();

    // (a) → ε (terminal)
    let eps = g.add_epsilon();
    let a = g.add_matcher(BuildMatcher::node("a"));
    g.connect(a, eps);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // Epsilon with no successors cannot be eliminated
    assert_eq!(stats.epsilons_kept, 1);
    assert!(dead.is_empty());
}

#[test]
fn quantifier_preserves_branch_structure() {
    let mut g = BuildGraph::new();

    // Typical zero_or_more structure: entry(branch) → [inner → branch, exit]
    let inner = g.matcher_fragment(BuildMatcher::node("item"));
    let _frag = g.zero_or_more(inner);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (item) → N1
    N1: ε → N0, N2
    N2: ε → ∅
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // Branch (N1) must remain, exit (N2) can't be eliminated (no successor)
    assert_eq!(stats.epsilons_kept, 2);
    assert_eq!(stats.epsilons_eliminated, 0);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (item) → N1
    N1: ε → N0, N2
    N2: ε → ∅
    "#);
}

#[test]
fn alternation_exit_epsilon_eliminated() {
    let mut g = BuildGraph::new();

    let f1 = g.matcher_fragment(BuildMatcher::node("a"));
    let f2 = g.matcher_fragment(BuildMatcher::node("b"));
    let frag = g.alternation(&[f1, f2]);

    // Add a successor to the exit so it can be eliminated
    let final_node = g.add_matcher(BuildMatcher::node("end"));
    g.connect(frag.exit, final_node);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (a) → N3
    N1: (b) → N3
    N2: ε → N0, N1
    N3: ε → N4
    N4: (end) → ∅
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // Exit epsilon (N3) should be eliminated, branch (N2) kept
    assert_eq!(stats.epsilons_eliminated, 1);
    assert_eq!(stats.epsilons_kept, 1);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (a) → N4
    N1: (b) → N4
    N2: ε → N0, N1
    N4: (end) → ∅
    "#);
}

#[test]
fn does_not_merge_effects_into_ref_marker() {
    let mut g = BuildGraph::new();

    // ε[Field] → ε+Exit(0) → (target)
    let target = g.add_matcher(BuildMatcher::node("target"));
    let exit = g.add_epsilon();
    g.node_mut(exit).set_ref_marker(RefMarker::exit(0));
    g.connect(exit, target);

    let field_eps = g.add_epsilon();
    g.node_mut(field_eps).add_effect(BuildEffect::Field("name"));
    g.connect(field_eps, exit);

    insta::assert_snapshot!(dump_graph(&g), @r#"
    N0: (target) → ∅
    N1: ε +Exit(0) → N0
    N2: ε [Field(name)] → N1
    "#);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // Should NOT merge Field effect into Exit node
    assert_eq!(stats.epsilons_kept, 2);
    assert_eq!(stats.epsilons_eliminated, 0);
    insta::assert_snapshot!(dump_live_graph(&g, &dead), @r#"
    N0: (target) → ∅
    N1: ε +Exit(0) → N0
    N2: ε [Field(name)] → N1
    "#);
}

#[test]
fn transfers_nav_to_stay_successor() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // ε[UpSkipTrivia(1)] → (target)[Stay]
    // Nav can be transferred to target, epsilon eliminated
    let target = g.add_matcher(BuildMatcher::node("end"));
    let up_epsilon = g.add_epsilon();
    g.node_mut(up_epsilon).set_nav(Nav::up_skip_trivia(1));
    g.connect(up_epsilon, target);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // Epsilon eliminated, nav transferred to target
    assert_eq!(stats.epsilons_eliminated, 1);
    assert!(dead.contains(&1));
    assert_eq!(g.node(0).nav, Nav::up_skip_trivia(1));
}

#[test]
fn keeps_epsilon_when_both_have_nav() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // ε[UpSkipTrivia(1)] → ε[UpSkipTrivia(1)] → (target)
    // Can't merge two non-Stay navs
    let target = g.add_matcher(BuildMatcher::node("end"));

    let up1 = g.add_epsilon();
    g.node_mut(up1).set_nav(Nav::up_skip_trivia(1));
    g.connect(up1, target);

    let up2 = g.add_epsilon();
    g.node_mut(up2).set_nav(Nav::up_skip_trivia(1));
    g.connect(up2, up1);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // First epsilon (up1) eliminated (successor has Stay)
    // Second epsilon (up2) kept (successor up1 has non-Stay nav)
    assert_eq!(stats.epsilons_eliminated, 1);
    assert_eq!(stats.epsilons_kept, 1);
    assert!(dead.contains(&1)); // up1 eliminated
    assert!(!dead.contains(&2)); // up2 kept
}

#[test]
fn eliminates_epsilon_with_stay_nav() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // ε[Stay] → (target) - Stay is the default, can be eliminated
    let target = g.add_matcher(BuildMatcher::node("target"));
    let eps = g.add_epsilon();
    g.node_mut(eps).set_nav(Nav::stay()); // explicit Stay
    g.connect(eps, target);

    let (dead, stats) = eliminate_epsilons(&mut g);

    assert_eq!(stats.epsilons_eliminated, 1);
    assert!(dead.contains(&1)); // epsilon was eliminated
}

#[test]
fn merges_unconstrained_up_levels() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // Simulates: ((((foo)))) - no anchors
    // ε[Up(1)] → ε[Up(1)] → ε[Up(1)] → (target)
    let target = g.add_matcher(BuildMatcher::node("end"));

    let up1 = g.add_epsilon();
    g.node_mut(up1).set_nav(Nav::up(1));
    g.connect(up1, target);

    let up2 = g.add_epsilon();
    g.node_mut(up2).set_nav(Nav::up(1));
    g.connect(up2, up1);

    let up3 = g.add_epsilon();
    g.node_mut(up3).set_nav(Nav::up(1));
    g.connect(up3, up2);

    let (_dead, stats) = eliminate_epsilons(&mut g);

    // All epsilons eliminated, levels merged into target
    assert_eq!(stats.epsilons_eliminated, 3);
    assert_eq!(g.node(0).nav, Nav::up(3));
}

#[test]
fn does_not_merge_constrained_up() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // Simulates: ((((foo) .) .) .) - anchors at each level
    // ε[UpSkipTrivia(1)] → ε[UpSkipTrivia(1)] → (target)
    let target = g.add_matcher(BuildMatcher::node("end"));

    let up1 = g.add_epsilon();
    g.node_mut(up1).set_nav(Nav::up_skip_trivia(1));
    g.connect(up1, target);

    let up2 = g.add_epsilon();
    g.node_mut(up2).set_nav(Nav::up_skip_trivia(1));
    g.connect(up2, up1);

    let (dead, stats) = eliminate_epsilons(&mut g);

    // First epsilon eliminated (transfers to target)
    // Second kept (can't merge UpSkipTrivia)
    assert_eq!(stats.epsilons_eliminated, 1);
    assert_eq!(stats.epsilons_kept, 1);
    assert!(dead.contains(&1));
    assert!(!dead.contains(&2));
}

#[test]
fn does_not_merge_mixed_up_kinds() {
    use crate::ir::Nav;

    let mut g = BuildGraph::new();

    // ε[Up(1)] → ε[UpSkipTrivia(1)] → (target)
    // Different Up kinds cannot merge
    let target = g.add_matcher(BuildMatcher::node("end"));

    let up1 = g.add_epsilon();
    g.node_mut(up1).set_nav(Nav::up_skip_trivia(1));
    g.connect(up1, target);

    let up2 = g.add_epsilon();
    g.node_mut(up2).set_nav(Nav::up(1)); // unconstrained
    g.connect(up2, up1);

    let (_dead, stats) = eliminate_epsilons(&mut g);

    // First epsilon eliminated (transfers to target)
    // Second kept (can't merge Up with UpSkipTrivia)
    assert_eq!(stats.epsilons_eliminated, 1);
    assert_eq!(stats.epsilons_kept, 1);
}
