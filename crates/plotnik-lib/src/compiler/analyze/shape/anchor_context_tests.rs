use rowan::TextRange;

use crate::compiler::parse::ast::QuantifierKind;

use super::anchor_context::{AnchorRangeRelation, ContextOutcome, ContextRelation};

#[test]
fn consuming_follower_discharges_only_the_trailing_side() {
    let relation = ContextRelation::anchor().then(&ContextRelation::atom());

    assert_eq!(
        relation,
        ContextRelation::singleton(ContextOutcome::ANCHOR.then(ContextOutcome::ATOM))
    );
    assert!(relation.requires_external_context());
}

#[test]
fn consuming_prefix_discharges_only_the_leading_side() {
    let relation = ContextRelation::atom().then(&ContextRelation::anchor());

    assert_eq!(
        relation,
        ContextRelation::singleton(ContextOutcome::ATOM.then(ContextOutcome::ANCHOR))
    );
    assert!(relation.requires_external_context());
}

#[test]
fn nullable_prefix_keeps_the_leading_anchor_path() {
    let optional = ContextRelation::atom().quantified(QuantifierKind::Optional);
    let relation = optional
        .then(&ContextRelation::anchor())
        .then(&ContextRelation::atom());

    assert!(relation.requires_external_context());
}

#[test]
fn both_consuming_neighbors_discharge_an_interior_anchor() {
    let relation = ContextRelation::atom()
        .then(&ContextRelation::anchor())
        .then(&ContextRelation::atom());

    assert_eq!(
        relation,
        ContextRelation::singleton(
            ContextOutcome::ATOM
                .then(ContextOutcome::ANCHOR)
                .then(ContextOutcome::ATOM)
        )
    );
    assert!(!relation.requires_external_context());
}

#[test]
fn repetition_exports_only_its_outer_iteration_boundaries() {
    let iteration = ContextRelation::anchor()
        .then(&ContextRelation::atom())
        .then(&ContextRelation::anchor());
    let repeated = iteration.quantified(QuantifierKind::OneOrMore);

    assert!(repeated.requires_external_context());
}

#[test]
fn quantifiers_prune_anchor_ranges_from_non_consuming_paths() {
    let pruned = TextRange::new(0.into(), 1.into());
    let retained = TextRange::new(2.into(), 3.into());
    let mut alternatives = AnchorRangeRelation::anchor(pruned);
    alternatives.union_with(&AnchorRangeRelation::atom());
    alternatives
        .union_with(&AnchorRangeRelation::anchor(retained).then(&AnchorRangeRelation::atom()));

    for kind in [
        QuantifierKind::Optional,
        QuantifierKind::ZeroOrMore,
        QuantifierKind::OneOrMore,
    ] {
        let quantified = alternatives.quantified(kind);

        assert_eq!(quantified.exported_ranges(), vec![retained]);
    }
}
