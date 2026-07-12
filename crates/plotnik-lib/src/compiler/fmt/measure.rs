use crate::compiler::parse::cst::SyntaxKind;

use super::ir::{
    CaptureCount, Element, GroupKind, InlineSummary, LayoutAnalysis, NodeKind, PrefixKind, Width,
};
use super::tokens::needs_space;

impl InlineSummary {
    fn token(kind: SyntaxKind, width: usize, captures: usize) -> Self {
        Self {
            width: Width(width),
            captures: CaptureCount(captures),
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
            captures: CaptureCount(self.captures.0.saturating_add(next.captures.0)),
            has_hardline: self.has_hardline || next.has_hardline,
            first_token: self.first_token.or(next.first_token),
            last_token: next.last_token.or(self.last_token),
            leading_comment: self.leading_comment,
            trailing_comment: next.trailing_comment,
            empty: false,
        }
    }
}

pub(super) fn analyze(kind: NodeKind, elements: &[Element]) -> LayoutAnalysis {
    let inline = elements
        .iter()
        .fold(InlineSummary::default(), |summary, element| {
            let next = match element {
                Element::Node(child) => child.analysis.inline,
                Element::Token(token) => InlineSummary::token(
                    token.kind,
                    token.text().chars().count(),
                    usize::from(matches!(
                        token.kind,
                        SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture
                    )),
                ),
                Element::Comment(comment) if comment.forces_line() => InlineSummary::hardline(),
                Element::Comment(comment) => InlineSummary::comment(comment.text().chars().count()),
            };
            summary.followed_by(next)
        });
    let child_break = elements.iter().any(|element| {
        element
            .node()
            .is_some_and(|child| child.analysis.must_break)
    });
    let group_item_count = |group: GroupKind| {
        elements
            .iter()
            .filter_map(Element::node)
            .filter(|child| group.contains_item(child.kind))
            .count()
    };
    let structural_break = match kind {
        NodeKind::Group(GroupKind::Alternation) => {
            let (count, labeled) = elements
                .iter()
                .filter_map(Element::node)
                .filter(|child| child.kind == NodeKind::Prefix(PrefixKind::Branch))
                .fold((0, false), |(count, labeled), branch| {
                    (
                        count + 1,
                        labeled || branch.direct_token(SyntaxKind::Id).is_some(),
                    )
                });
            count >= 2 || labeled
        }
        NodeKind::Group(group @ GroupKind::Sequence(_)) => group_item_count(group) >= 2,
        NodeKind::Group(group @ GroupKind::NamedNode) => {
            elements
                .iter()
                .filter_map(Element::node)
                .filter(|child| group.contains_item(child.kind))
                .filter(|child| child.kind != NodeKind::Anchor)
                .count()
                >= 2
        }
        _ => false,
    };
    let must_break =
        structural_break || inline.captures.0 >= 2 || inline.has_hardline || child_break;
    LayoutAnalysis { inline, must_break }
}
