use std::collections::{BTreeMap, HashSet};

use super::compute_invalid_containment;
use crate::compiler::analyze::types::type_shape::{RecordField, TypeId, TypeShape};
use crate::compiler::ids::DefId;
use crate::core::Interner;

#[test]
fn invalid_containment_propagates_through_recursive_cycle() {
    let mut interner = Interner::new();
    let cycle = interner.intern("cycle");
    let bad = interner.intern("bad");
    let recursive = TypeId(4);
    let reference = TypeId(5);
    let invalid = TypeId(6);
    let definition = DefId::from_raw(0);
    let types = vec![
        TypeShape::NoValue,
        TypeShape::Node,
        TypeShape::Text,
        TypeShape::Bool,
        TypeShape::Record(BTreeMap::from([
            (cycle, RecordField::new(reference)),
            (bad, RecordField::new(invalid)),
        ])),
        TypeShape::Ref(definition),
        TypeShape::Custom(interner.intern("Invalid")),
    ];
    let definitions = BTreeMap::from([(definition, recursive)]);

    let contained = compute_invalid_containment(&types, &definitions, &HashSet::from([invalid]));

    assert!(contained.contains(&invalid));
    assert!(contained.contains(&recursive));
    assert!(contained.contains(&reference));
}
