use crate::compiler::parse::ast;

/// Whether a pattern position must participate in every match.
///
/// Positions under a disjunction branch or a zero-width quantifier body are deferred:
/// a sibling branch, or zero repetitions, can satisfy the enclosing pattern without
/// this position participating. A `+` quantifier keeps the incoming participation
/// because its body must match at least once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Participation {
    Required,
    Deferred,
}

impl Participation {
    pub(super) fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }

    pub(super) fn inside_alternative(self) -> Self {
        Self::Deferred
    }

    pub(super) fn inside_quantifier_body(self, quantifier: &ast::QuantifiedPattern) -> Self {
        if quantifier.is_optional() {
            Self::Deferred
        } else {
            self
        }
    }
}
