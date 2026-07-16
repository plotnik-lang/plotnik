use super::boundary::{
    BoundaryOutcome, BoundaryRelation, BoundaryState, FirstClass, PendingAnchor,
};

fn outcome(previous: FirstClass, pending: PendingAnchor, consumed: bool) -> BoundaryOutcome {
    BoundaryOutcome {
        state: BoundaryState::new(previous, pending),
        consumed,
        first: if consumed {
            previous
        } else {
            FirstClass::Empty
        },
    }
}

#[test]
fn consecutive_anchors_tighten() {
    let relation = BoundaryRelation::identity()
        .anchor(PendingAnchor::Exact)
        .anchor(PendingAnchor::Soft);

    assert_eq!(
        relation.outcomes(BoundaryState::START),
        &[outcome(FirstClass::Empty, PendingAnchor::Exact, false)]
            .into_iter()
            .collect()
    );
}

#[test]
fn wildcard_is_one_either_outcome_not_two_alternatives() {
    let relation = BoundaryRelation::atom(FirstClass::Either);

    assert_eq!(
        relation.outcomes(BoundaryState::START),
        &[outcome(FirstClass::Either, PendingAnchor::None, true)]
            .into_iter()
            .collect()
    );
}

#[test]
fn optional_prunes_empty_inner_iterations_but_keeps_zero_iteration_identity() {
    let inner = BoundaryRelation::identity().anchor(PendingAnchor::Soft);
    let optional = inner.quantified(crate::compiler::parse::ast::QuantifierKind::Optional);

    assert_eq!(
        optional.outcomes(BoundaryState::START),
        &[outcome(FirstClass::Empty, PendingAnchor::None, false)]
            .into_iter()
            .collect()
    );
}

#[test]
fn star_closure_only_loops_over_consuming_outcomes() {
    let mut inner = BoundaryRelation::atom(FirstClass::Named);
    inner.union_with(&BoundaryRelation::identity().anchor(PendingAnchor::Exact));
    let star = inner.quantified(crate::compiler::parse::ast::QuantifierKind::ZeroOrMore);

    assert_eq!(
        star.outcomes(BoundaryState::START),
        &[
            outcome(FirstClass::Empty, PendingAnchor::None, false),
            outcome(FirstClass::Named, PendingAnchor::None, true),
        ]
        .into_iter()
        .collect()
    );
}
