use crate::compiler::parse::cst::SyntaxKind;

use super::ir::{
    Element, GroupKind, GroupPart, InlineSummary, LandmarkCount, LayoutAnalysis, NodeKind,
    NodeLayout, Width,
};
use super::tokens::needs_space;

impl InlineSummary {
    fn token(kind: SyntaxKind, width: usize) -> Self {
        Self {
            width: Width(width),
            first_token: Some(kind),
            last_token: Some(kind),
            empty: false,
            ..Self::default()
        }
    }

    fn comment(width: usize) -> Self {
        Self {
            width: Width(width),
            leading_comment: true,
            trailing_comment: true,
            empty: false,
            ..Self::default()
        }
    }

    fn hardline() -> Self {
        Self {
            has_hardline: true,
            ..Self::default()
        }
    }

    fn followed_by(self, next: Self) -> Self {
        if self.empty {
            return Self {
                has_hardline: self.has_hardline || next.has_hardline,
                ..next
            };
        }
        if next.empty {
            return Self {
                has_hardline: self.has_hardline || next.has_hardline,
                ..self
            };
        }
        let separator = usize::from(
            self.trailing_comment
                || next.leading_comment
                || next
                    .first_token
                    .is_some_and(|current| needs_space(self.last_token, current)),
        );
        Self {
            width: Width(
                self.width
                    .0
                    .saturating_add(separator)
                    .saturating_add(next.width.0),
            ),
            landmarks: LandmarkCount(self.landmarks.0.saturating_add(next.landmarks.0)),
            has_hardline: self.has_hardline || next.has_hardline,
            first_token: self.first_token.or(next.first_token),
            last_token: next.last_token.or(self.last_token),
            leading_comment: self.leading_comment,
            trailing_comment: next.trailing_comment,
            empty: false,
        }
    }

    fn with_landmarks(mut self, count: usize) -> Self {
        self.landmarks = LandmarkCount(self.landmarks.0.saturating_add(count));
        self
    }
}

pub(super) fn analyze(kind: NodeKind, layout: &NodeLayout, elements: &[Element]) -> LayoutAnalysis {
    let inline = elements
        .iter()
        .fold(InlineSummary::default(), |summary, element| {
            let next = match element {
                Element::Node(child) => child.analysis.inline,
                Element::Token(token) => {
                    InlineSummary::token(token.kind, token.text().chars().count())
                }
                Element::Comment(comment) if comment.forces_line() => InlineSummary::hardline(),
                Element::Comment(comment) => InlineSummary::comment(comment.text().chars().count()),
            };
            summary.followed_by(next)
        })
        .with_landmarks(kind.landmark_count());
    let child_break = elements.iter().any(|element| {
        element
            .node()
            .is_some_and(|child| child.analysis.must_break)
    });
    let structural_break = match layout {
        NodeLayout::Group(group) if group.kind == GroupKind::Alternation => {
            let (count, labeled) = group
                .parts
                .iter()
                .filter_map(|part| match part {
                    GroupPart::Item(index) => elements[*index].node(),
                    GroupPart::Comment(_) => None,
                })
                .fold((0, false), |(count, labeled), alternative| {
                    (
                        count + 1,
                        labeled || alternative.direct_token(SyntaxKind::Id).is_some(),
                    )
                });
            count >= 2 || labeled
        }
        NodeLayout::Group(group) if matches!(group.kind, GroupKind::Sequence(_)) => {
            group.item_count() >= 2
        }
        NodeLayout::Group(group) if group.kind == GroupKind::NamedNode => {
            group
                .parts
                .iter()
                .filter_map(|part| match part {
                    GroupPart::Item(index) => elements[*index].node(),
                    GroupPart::Comment(_) => None,
                })
                .filter(|child| child.kind != NodeKind::Anchor)
                .count()
                >= 2
        }
        _ => false,
    };
    let must_break =
        structural_break || inline.landmarks.0 > 3 || inline.has_hardline || child_break;
    LayoutAnalysis { inline, must_break }
}
