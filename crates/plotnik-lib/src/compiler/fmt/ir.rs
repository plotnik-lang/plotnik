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
            SyntaxKind::Branch => Self::Prefix(PrefixKind::Branch),
            SyntaxKind::Capture => Self::Suffix(SuffixKind::Capture),
            SyntaxKind::Quantifier => Self::Suffix(SuffixKind::Quantifier),
            SyntaxKind::DefRef | SyntaxKind::Str | SyntaxKind::Wildcard => Self::PatternAtom,
            SyntaxKind::Anchor => Self::Anchor,
            SyntaxKind::NegatedField => Self::NegatedField,
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
            Self::Alternation => child == NodeKind::Prefix(PrefixKind::Branch),
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
    Branch,
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
    pub analysis: LayoutAnalysis,
}

impl ModelNode {
    pub fn children(&self) -> impl Iterator<Item = &ModelNode> {
        self.elements.iter().filter_map(Element::node)
    }

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

    pub fn group_kind(&self) -> Option<GroupKind> {
        let NodeKind::Group(kind) = self.kind else {
            return None;
        };
        Some(kind)
    }

    pub fn is_group_item(&self, child: &ModelNode) -> bool {
        self.group_kind()
            .is_some_and(|group| group.contains_item(child.kind))
    }
}

#[derive(Debug)]
pub(super) struct FormatFile {
    pub root: ModelNode,
    pub comment_count: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Width(pub usize);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct CaptureCount(pub usize);

#[derive(Debug, Clone, Copy)]
pub(super) struct InlineSummary {
    pub width: Width,
    pub captures: CaptureCount,
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
            captures: CaptureCount::default(),
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
