use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::cst::SyntaxToken;

use super::comments::Comment;

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub kind: SyntaxKind,
    syntax: SyntaxToken,
    replacement: Option<&'static str>,
}

impl Token {
    pub fn new(syntax: SyntaxToken, replacement: Option<&'static str>) -> Self {
        Self {
            kind: syntax.kind(),
            syntax,
            replacement,
        }
    }

    pub fn text(&self) -> &str {
        self.replacement.unwrap_or_else(|| self.syntax.text())
    }
}

#[derive(Debug, Clone)]
pub(super) enum Element {
    Node(Box<ModelNode>),
    Token(Token),
    Comment(Comment),
}

impl Element {
    pub fn node(&self) -> Option<&ModelNode> {
        match self {
            Self::Node(node) => Some(node),
            Self::Token(_) | Self::Comment(_) => None,
        }
    }

    pub fn comment(&self) -> Option<&Comment> {
        match self {
            Self::Comment(comment) => Some(comment),
            Self::Node(_) | Self::Token(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeKind {
    Root,
    Definition,
    Group(GroupKind),
    Prefix(PrefixKind),
    Suffix(SuffixKind),
    PatternAtom,
    Anchor,
    NegatedField,
    Predicate,
    CaptureType,
    Atomic,
}

impl NodeKind {
    pub fn from_syntax(kind: SyntaxKind, elements: &[Element]) -> Self {
        match kind {
            SyntaxKind::Root => Self::Root,
            SyntaxKind::Def => Self::Definition,
            SyntaxKind::NamedNode => Self::Group(GroupKind::NamedNode),
            SyntaxKind::Sequence => {
                let delimiter = if has_direct_token(elements, SyntaxKind::BraceOpen) {
                    SequenceDelimiter::Braces
                } else {
                    SequenceDelimiter::Parentheses
                };
                Self::Group(GroupKind::Sequence(delimiter))
            }
            SyntaxKind::Alternation => Self::Group(GroupKind::Alternation),
            SyntaxKind::Field => Self::Prefix(PrefixKind::Field),
            SyntaxKind::Alternative => Self::Prefix(PrefixKind::Alternative {
                labeled: has_direct_token(elements, SyntaxKind::Id),
            }),
            SyntaxKind::Capture => Self::Suffix(SuffixKind::Capture),
            SyntaxKind::Quantifier => Self::Suffix(SuffixKind::Quantifier),
            SyntaxKind::DefRef | SyntaxKind::Str | SyntaxKind::Wildcard => Self::PatternAtom,
            SyntaxKind::Anchor => Self::Anchor,
            SyntaxKind::NegatedField => Self::NegatedField,
            SyntaxKind::NodePredicate => Self::Predicate,
            SyntaxKind::CaptureType => Self::CaptureType,
            _ => Self::Atomic,
        }
    }

    pub fn is_pattern(self) -> bool {
        matches!(
            self,
            Self::Group(_) | Self::Prefix(PrefixKind::Field) | Self::Suffix(_) | Self::PatternAtom
        )
    }

    pub fn is_definition_body(self) -> bool {
        self.is_pattern() || matches!(self, Self::Anchor | Self::NegatedField)
    }

    pub fn landmark_count(self) -> usize {
        match self {
            Self::Root | Self::Definition | Self::Atomic => 0,
            Self::Prefix(PrefixKind::Alternative { labeled: false }) => 0,
            Self::Group(_)
            | Self::Prefix(_)
            | Self::Suffix(_)
            | Self::PatternAtom
            | Self::Anchor
            | Self::NegatedField
            | Self::Predicate
            | Self::CaptureType => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupKind {
    NamedNode,
    Sequence(SequenceDelimiter),
    Alternation,
}

impl GroupKind {
    pub fn close_token(self) -> SyntaxKind {
        match self {
            Self::NamedNode | Self::Sequence(SequenceDelimiter::Parentheses) => {
                SyntaxKind::ParenClose
            }
            Self::Sequence(SequenceDelimiter::Braces) => SyntaxKind::BraceClose,
            Self::Alternation => SyntaxKind::BracketClose,
        }
    }

    pub fn contains_item(self, child: NodeKind) -> bool {
        match self {
            Self::Alternation => {
                matches!(child, NodeKind::Prefix(PrefixKind::Alternative { .. }))
            }
            Self::NamedNode | Self::Sequence(_) => {
                child.is_pattern() || matches!(child, NodeKind::Anchor | NodeKind::NegatedField)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SequenceDelimiter {
    Parentheses,
    Braces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrefixKind {
    Field,
    Alternative { labeled: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SuffixKind {
    Capture,
    Quantifier,
}

#[derive(Debug, Clone)]
pub(super) struct ModelNode {
    pub kind: NodeKind,
    pub elements: Vec<Element>,
    pub layout: NodeLayout,
    pub analysis: LayoutAnalysis,
}

impl ModelNode {
    pub fn for_each_descendant_comment(&self, visit: &mut impl FnMut(&Comment)) {
        for element in &self.elements {
            match element {
                Element::Node(node) => node.for_each_descendant_comment(visit),
                Element::Comment(comment) => visit(comment),
                Element::Token(_) => {}
            }
        }
    }

    pub fn direct_token(&self, kind: SyntaxKind) -> Option<&Token> {
        self.elements.iter().find_map(|element| match element {
            Element::Token(token) if token.kind == kind => Some(token),
            _ => None,
        })
    }

    pub fn node_at(&self, index: usize) -> &ModelNode {
        self.elements[index]
            .node()
            .expect("layout node boundary points at a node")
    }

    pub fn comment_at(&self, index: usize) -> &Comment {
        self.elements[index]
            .comment()
            .expect("layout comment boundary points at a comment")
    }

    pub fn fragment(&self, fragment: Fragment) -> &[Element] {
        &self.elements[fragment.range()]
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Fragment {
    start: usize,
    end: usize,
}

impl Fragment {
    pub fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "fragment boundaries are ordered");
        Self { start, end }
    }

    pub fn before(index: usize) -> Self {
        Self::new(0, index)
    }

    pub fn after(index: usize, len: usize) -> Self {
        Self::new(index + 1, len)
    }

    fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Debug, Clone)]
pub(super) enum NodeLayout {
    Root { parts: Vec<FilePart> },
    Definition { prefix: Fragment, body: usize },
    Group(GroupLayout),
    Prefix { prefix: Fragment, body: usize },
    Suffix { body: usize, suffix: Fragment },
    Atomic,
}

impl NodeLayout {
    pub fn for_node(kind: NodeKind, elements: &[Element]) -> Self {
        match kind {
            NodeKind::Root => Self::Root {
                parts: elements
                    .iter()
                    .enumerate()
                    .filter_map(|(index, element)| match element {
                        Element::Node(node) if node.kind == NodeKind::Definition => {
                            Some(FilePart::Definition(index))
                        }
                        Element::Comment(_) => Some(FilePart::Comment(index)),
                        Element::Token(token) if token.kind == SyntaxKind::Shebang => {
                            Some(FilePart::Shebang(index))
                        }
                        Element::Node(_) | Element::Token(_) => None,
                    })
                    .collect(),
            },
            NodeKind::Definition => {
                let body = find_child(elements, NodeKind::is_definition_body);
                Self::Definition {
                    prefix: Fragment::before(body),
                    body,
                }
            }
            NodeKind::Group(group) => Self::Group(GroupLayout::new(group, elements)),
            NodeKind::Prefix(_) => {
                let body = find_child(elements, NodeKind::is_pattern);
                Self::Prefix {
                    prefix: Fragment::before(body),
                    body,
                }
            }
            NodeKind::Suffix(_) => {
                let body = find_child(elements, NodeKind::is_pattern);
                Self::Suffix {
                    body,
                    suffix: Fragment::after(body, elements.len()),
                }
            }
            NodeKind::PatternAtom
            | NodeKind::Anchor
            | NodeKind::NegatedField
            | NodeKind::Predicate
            | NodeKind::CaptureType
            | NodeKind::Atomic => Self::Atomic,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FilePart {
    Definition(usize),
    Comment(usize),
    Shebang(usize),
}

#[derive(Debug, Clone)]
pub(super) struct GroupLayout {
    pub kind: GroupKind,
    pub head: Fragment,
    pub parts: Vec<GroupPart>,
    pub closer: usize,
}

impl GroupLayout {
    fn new(kind: GroupKind, elements: &[Element]) -> Self {
        let closer = elements
            .iter()
            .rposition(|element| {
                matches!(element, Element::Token(token) if token.kind == kind.close_token())
            })
            .expect("parse-clean group has a closer");
        let first_item = elements.iter().enumerate().find_map(|(index, element)| {
            element
                .node()
                .filter(|child| kind.contains_item(child.kind))
                .map(|_| index)
        });
        let head_end = first_item.unwrap_or_else(|| {
            elements[..closer]
                .iter()
                .position(|element| element.comment().is_some_and(Comment::forces_line))
                .unwrap_or(closer)
        });
        let parts = elements[head_end..closer]
            .iter()
            .enumerate()
            .filter_map(|(offset, element)| {
                let index = head_end + offset;
                match element {
                    Element::Node(child) if kind.contains_item(child.kind) => {
                        Some(GroupPart::Item(index))
                    }
                    Element::Comment(_) => Some(GroupPart::Comment(index)),
                    Element::Node(_) | Element::Token(_) => None,
                }
            })
            .collect();
        Self {
            kind,
            head: Fragment::before(head_end),
            parts,
            closer,
        }
    }

    pub fn has_items(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, GroupPart::Item(_)))
    }

    pub fn item_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|part| matches!(part, GroupPart::Item(_)))
            .count()
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum GroupPart {
    Item(usize),
    Comment(usize),
}

#[derive(Debug)]
pub(super) struct FormatFile {
    pub root: ModelNode,
    pub comment_count: usize,
    pub source_len: usize,
    pub normalization_work: usize,
}

#[derive(Debug, Default)]
pub(super) struct WorkCounter {
    #[cfg(test)]
    value: usize,
}

impl WorkCounter {
    #[inline]
    pub fn add(&mut self, amount: usize) {
        #[cfg(test)]
        {
            self.value += amount;
        }
        #[cfg(not(test))]
        let _ = amount;
    }

    pub fn value(&self) -> usize {
        #[cfg(test)]
        {
            self.value
        }
        #[cfg(not(test))]
        {
            0
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(super) struct FormatMetrics {
    pub work: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Width(pub usize);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct LandmarkCount(pub usize);

#[derive(Debug, Clone, Copy)]
pub(super) struct InlineSummary {
    pub width: Width,
    pub landmarks: LandmarkCount,
    pub has_hardline: bool,
    pub first_token: Option<SyntaxKind>,
    pub last_token: Option<SyntaxKind>,
    pub leading_comment: bool,
    pub trailing_comment: bool,
    pub empty: bool,
}

impl Default for InlineSummary {
    fn default() -> Self {
        Self {
            width: Width::default(),
            landmarks: LandmarkCount::default(),
            has_hardline: false,
            first_token: None,
            last_token: None,
            leading_comment: false,
            trailing_comment: false,
            empty: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct LayoutAnalysis {
    pub inline: InlineSummary,
    pub must_break: bool,
}

fn has_direct_token(elements: &[Element], kind: SyntaxKind) -> bool {
    elements
        .iter()
        .any(|element| matches!(element, Element::Token(token) if token.kind == kind))
}

fn find_child(elements: &[Element], predicate: impl Fn(NodeKind) -> bool) -> usize {
    elements
        .iter()
        .position(|element| element.node().is_some_and(|child| predicate(child.kind)))
        .expect("parse-clean wrapper has its semantic body")
}
use std::ops::Range;
