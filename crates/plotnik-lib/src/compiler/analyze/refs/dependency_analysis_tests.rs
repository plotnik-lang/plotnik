use crate::compiler::ids::DefId;

use super::dependency_analysis::DefinitionDependencies;

#[test]
fn reachability_is_stable_across_recursive_edges_and_omits_unused_definitions() {
    let first = DefId::from_raw(0);
    let second = DefId::from_raw(1);
    let unused = DefId::from_raw(2);
    let dependencies = DefinitionDependencies::new(vec![vec![second], vec![first], Vec::new()]);

    let reachable = dependencies.reachable_from([first]);

    assert_eq!(reachable.iter().collect::<Vec<_>>(), vec![first, second]);
    assert!(reachable.contains(first));
    assert!(reachable.contains(second));
    assert!(!reachable.contains(unused));
}

#[test]
fn inbound_usage_is_derived_from_the_same_outgoing_graph() {
    let entrypoint = DefId::from_raw(0);
    let fragment = DefId::from_raw(1);
    let dependencies = DefinitionDependencies::new(vec![vec![fragment], Vec::new()]);

    assert!(!dependencies.has_inbound_references(entrypoint));
    assert!(dependencies.has_inbound_references(fragment));
}
