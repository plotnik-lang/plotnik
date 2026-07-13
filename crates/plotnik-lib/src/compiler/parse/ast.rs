//! Typed AST wrappers over CST nodes.
//!
//! Each struct wraps a `SyntaxNode` and provides typed accessors.
//! Cast is infallible for correct `SyntaxKind` - validation happens elsewhere.
//!
//! ## String Lifetime Limitation
//!
//! `SyntaxToken::text()` returns `&str` tied to the token's lifetime, not to the
//! source `&'q str`. This is a rowan design: tokens store interned strings, not
//! spans into the original source.
//!
//! When building data structures that need source-lifetime strings (e.g.,
//! `SymbolTable<'q>`), use [`token_src`] instead of `token.text()`.

use crate::compiler::parse::cst::{QueryLang, SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::TextRange;

/// Extracts token text with source lifetime.
///
/// Use this instead of `token.text()` when you need `&'q str`.
pub fn token_src<'q>(token: &SyntaxToken, source: &'q str) -> &'q str {
    let range = token.text_range();
    &source[range.start().into()..range.end().into()]
}

fn find_token(node: &SyntaxNode, pred: impl Fn(SyntaxKind) -> bool) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|t| pred(t.kind()))
}

macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident, $($kind:ident)|+) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        // Uniform generated AST wrapper API; not every wrapper currently needs
        // every accessor directly, but keeping the shape identical avoids
        // one-off wrapper special cases.
        #[allow(dead_code)]
        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then(|| Self(node))
            }

            pub fn can_cast(kind: SyntaxKind) -> bool {
                matches!(kind, $(SyntaxKind::$kind)|+)
            }

            pub fn syntax(&self) -> &SyntaxNode {
                &self.0
            }

            pub fn text_range(&self) -> TextRange {
                self.0.text_range()
            }
        }
    };
}

macro_rules! define_pattern {
    ($($variant:ident),+ $(,)?) => {
        /// Pattern: any construct that can appear in the tree.
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum Pattern {
            $($variant($variant),)+
            /// Alternation `[a b]` or `[A: a B: b]`.
            Alternation(AlternationPattern),
        }

        impl Pattern {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                let kind = node.kind();
                if kind == SyntaxKind::Alternation {
                    return Some(Pattern::Alternation(AlternationPattern(node)));
                }
                $(if $variant::can_cast(kind) { return Some(Pattern::$variant($variant(node))); })+
                None
            }

            pub fn syntax(&self) -> &SyntaxNode {
                match self {
                    $(Pattern::$variant(n) => n.syntax(),)+
                    Pattern::Alternation(n) => n.syntax(),
                }
            }

            pub fn text_range(&self) -> TextRange {
                match self {
                    $(Pattern::$variant(n) => n.text_range(),)+
                    Pattern::Alternation(n) => n.text_range(),
                }
            }
        }
    };
}

impl Pattern {
    pub fn children(&self) -> impl Iterator<Item = Pattern> + '_ {
        match self {
            Pattern::NodePattern(n) => {
                PatternChildren::Direct(n.syntax().children().filter_map(Pattern::cast))
            }
            Pattern::SeqPattern(s) => {
                PatternChildren::Direct(s.syntax().children().filter_map(Pattern::cast))
            }
            Pattern::CapturedPattern(c) => PatternChildren::Optional(c.inner().into_iter()),
            Pattern::QuantifiedPattern(q) => PatternChildren::Optional(q.inner().into_iter()),
            Pattern::FieldPattern(f) => PatternChildren::Optional(f.value().into_iter()),
            Pattern::Alternation(a) => PatternChildren::AlternativeBodies(
                a.syntax()
                    .children()
                    .filter_map(Alternative::cast as fn(SyntaxNode) -> Option<Alternative>)
                    .filter_map(alternative_body as fn(Alternative) -> Option<Pattern>),
            ),
            Pattern::DefRef(_) | Pattern::TokenPattern(_) => {
                PatternChildren::Empty(std::iter::empty())
            }
        }
    }
}

type DirectPatternChildren =
    std::iter::FilterMap<rowan::SyntaxNodeChildren<QueryLang>, fn(SyntaxNode) -> Option<Pattern>>;
type AlternativeChildren = std::iter::FilterMap<
    rowan::SyntaxNodeChildren<QueryLang>,
    fn(SyntaxNode) -> Option<Alternative>,
>;
type AlternativeBodyChildren =
    std::iter::FilterMap<AlternativeChildren, fn(Alternative) -> Option<Pattern>>;

enum PatternChildren {
    Direct(DirectPatternChildren),
    Optional(std::option::IntoIter<Pattern>),
    AlternativeBodies(AlternativeBodyChildren),
    Empty(std::iter::Empty<Pattern>),
}

impl Iterator for PatternChildren {
    type Item = Pattern;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Direct(children) => children.next(),
            Self::Optional(child) => child.next(),
            Self::AlternativeBodies(children) => children.next(),
            Self::Empty(children) => children.next(),
        }
    }
}

fn alternative_body(alternative: Alternative) -> Option<Pattern> {
    alternative.body()
}

ast_node!(Root, Root);
ast_node!(Def, Def);
ast_node!(NodePattern, NamedNode);
ast_node!(DefRef, DefRef);
ast_node!(
    /// Alternation `[a b]` or `[A: a B: b]`.
    AlternationPattern,
    Alternation
);
ast_node!(Alternative, Alternative);
ast_node!(SeqPattern, Sequence);
ast_node!(CapturedPattern, Capture);
ast_node!(CaptureTypeSyntax, CaptureType);
ast_node!(QuantifiedPattern, Quantifier);
ast_node!(FieldPattern, Field);
ast_node!(NegatedField, NegatedField);
ast_node!(Anchor, Anchor);
ast_node!(NodePredicate, NodePredicate);
ast_node!(RegexLiteral, Regex);

impl Anchor {
    pub fn is_exact(&self) -> bool {
        find_token(&self.0, |k| k == SyntaxKind::DotBang).is_some()
    }
}

/// Either a pattern or an anchor in a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SeqItem {
    Pattern(Pattern),
    Anchor(Anchor),
}

impl SeqItem {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        if Anchor::can_cast(node.kind()) {
            return Anchor::cast(node).map(SeqItem::Anchor);
        }
        Pattern::cast(node).map(SeqItem::Pattern)
    }

    pub fn as_pattern(&self) -> Option<&Pattern> {
        match self {
            SeqItem::Pattern(e) => Some(e),
            _ => None,
        }
    }
}

ast_node!(
    /// Token pattern: an anonymous token (`"+"`) or the wildcard (`_`).
    /// Maps from CST `Str` or `Wildcard`.
    TokenPattern,
    Str | Wildcard
);

impl TokenPattern {
    /// Returns the token's string value, `None` if this is the wildcard.
    pub fn value(&self) -> Option<SyntaxToken> {
        if self.0.kind() == SyntaxKind::Wildcard {
            return None;
        }
        find_token(&self.0, |k| k == SyntaxKind::StringContent)
    }

    pub fn is_any(&self) -> bool {
        self.0.kind() == SyntaxKind::Wildcard
    }
}

/// Whether an alternation's alternatives are consistently labeled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Labeling {
    /// No alternatives have labels: `[expr1 expr2]`.
    Unlabeled,
    /// All alternatives have labels: `[A: expr1 B: expr2]`.
    Labeled,
    /// Only some alternatives have labels (invalid).
    Mixed,
}

fn classify_labeling(node: &SyntaxNode) -> Labeling {
    let mut has_labeled = false;
    let mut has_unlabeled = false;

    for child in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::Alternative)
    {
        if find_token(&child, |k| k == SyntaxKind::Id).is_some() {
            has_labeled = true;
        } else {
            has_unlabeled = true;
        }
    }

    match (has_labeled, has_unlabeled) {
        (true, true) => Labeling::Mixed,
        (true, false) => Labeling::Labeled,
        _ => Labeling::Unlabeled,
    }
}

/// Syntactic predicate operator parsed from `(node OP value)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateOperator {
    Eq,
    Ne,
    StartsWith,
    EndsWith,
    Contains,
    RegexMatch,
    RegexNoMatch,
}

impl PredicateOperator {
    pub fn is_regex_op(self) -> bool {
        matches!(self, Self::RegexMatch | Self::RegexNoMatch)
    }
}

fn predicate_op_from_syntax_kind(kind: SyntaxKind) -> Option<PredicateOperator> {
    match kind {
        SyntaxKind::OpEq => Some(PredicateOperator::Eq),
        SyntaxKind::OpNe => Some(PredicateOperator::Ne),
        SyntaxKind::OpStartsWith => Some(PredicateOperator::StartsWith),
        SyntaxKind::OpEndsWith => Some(PredicateOperator::EndsWith),
        SyntaxKind::OpContains => Some(PredicateOperator::Contains),
        SyntaxKind::OpRegexMatch => Some(PredicateOperator::RegexMatch),
        SyntaxKind::OpRegexNoMatch => Some(PredicateOperator::RegexNoMatch),
        _ => None,
    }
}

/// Syntactic quantifier arity parsed from `?`, `*`, `+`, and lazy twins.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuantifierKind {
    /// `?` or `??` - zero or one.
    Optional,
    /// `*` or `*?` - zero or more.
    ZeroOrMore,
    /// `+` or `+?` - one or more.
    OneOrMore,
}

impl QuantifierKind {
    pub fn is_non_empty(self) -> bool {
        matches!(self, Self::OneOrMore)
    }
}

/// Syntactic quantifier greediness parsed from a quantifier token.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Greediness {
    Greedy,
    Lazy,
}

impl Greediness {
    pub fn is_greedy(self) -> bool {
        matches!(self, Self::Greedy)
    }
}

/// Syntactic quantifier operator parsed from `?`, `*`, `+`, and lazy twins.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct QuantifierOperator {
    kind: QuantifierKind,
    greediness: Greediness,
}

impl QuantifierOperator {
    pub fn new(kind: QuantifierKind, greediness: Greediness) -> Self {
        Self { kind, greediness }
    }

    pub fn kind(self) -> QuantifierKind {
        self.kind
    }

    pub fn is_greedy(self) -> bool {
        self.greediness.is_greedy()
    }
}

fn quantifier_operator_from_syntax_kind(kind: SyntaxKind) -> Option<QuantifierOperator> {
    Some(match kind {
        SyntaxKind::Question => {
            QuantifierOperator::new(QuantifierKind::Optional, Greediness::Greedy)
        }
        SyntaxKind::QuestionQuestion => {
            QuantifierOperator::new(QuantifierKind::Optional, Greediness::Lazy)
        }
        SyntaxKind::Star => QuantifierOperator::new(QuantifierKind::ZeroOrMore, Greediness::Greedy),
        SyntaxKind::StarQuestion => {
            QuantifierOperator::new(QuantifierKind::ZeroOrMore, Greediness::Lazy)
        }
        SyntaxKind::Plus => QuantifierOperator::new(QuantifierKind::OneOrMore, Greediness::Greedy),
        SyntaxKind::PlusQuestion => {
            QuantifierOperator::new(QuantifierKind::OneOrMore, Greediness::Lazy)
        }
        _ => return None,
    })
}

define_pattern!(
    NodePattern,
    DefRef,
    TokenPattern,
    SeqPattern,
    CapturedPattern,
    QuantifiedPattern,
    FieldPattern,
);

impl Root {
    pub fn defs(&self) -> impl Iterator<Item = Def> + '_ {
        self.0.children().filter_map(Def::cast)
    }

    pub fn patterns(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.0.children().filter_map(Pattern::cast)
    }
}

impl Def {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }
}

/// The kind argument of a `(MISSING …)` pattern — the token naming the specific
/// kind the inserted node stands in for. Bare `(MISSING)` has none.
pub enum MissingArg {
    /// `(MISSING identifier)` — the `Id` token of a named kind.
    Named(SyntaxToken),
    /// `(MISSING ";")` — the `StringContent` token of an anonymous kind.
    Anonymous(SyntaxToken),
}

impl NodePattern {
    pub fn kind_token(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| {
            matches!(
                k,
                SyntaxKind::Id
                    | SyntaxKind::Underscore
                    | SyntaxKind::KwError
                    | SyntaxKind::KwMissing
            )
        })
    }

    /// True when the node carries a supertype marker — `#`, or the deprecated `/` — whether or
    /// not a subtype follows it. Both `(expression#)` and `(expression#binary_expression)` are
    /// true; a plain `(expression)` is false. Distinguishes an explicit supertype match from a
    /// bare node that happens to name a supertype.
    pub fn has_supertype_marker(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|t| matches!(t.kind(), SyntaxKind::Hash | SyntaxKind::Slash))
    }

    pub fn is_any(&self) -> bool {
        self.kind_token()
            .map(|t| t.kind() == SyntaxKind::Underscore)
            .unwrap_or(false)
    }

    pub fn is_missing(&self) -> bool {
        self.kind_token()
            .map(|t| t.kind() == SyntaxKind::KwMissing)
            .unwrap_or(false)
    }

    /// The kind argument inside `(MISSING <arg>)`, or `None` for bare `(MISSING)`
    /// and any non-MISSING node. `(MISSING identifier)` yields `Named`, `(MISSING ";")`
    /// yields `Anonymous`. The argument is a raw token, not a nested pattern (the
    /// parser forbids MISSING children), so it is read directly off this node.
    pub fn missing_arg(&self) -> Option<MissingArg> {
        if !self.is_missing() {
            return None;
        }
        if let Some(id) = find_token(&self.0, |k| k == SyntaxKind::Id) {
            return Some(MissingArg::Named(id));
        }
        let content = find_token(&self.0, |k| k == SyntaxKind::StringContent)?;
        Some(MissingArg::Anonymous(content))
    }

    /// Returns the predicate if present: `(identifier == "foo")`.
    pub fn predicate(&self) -> Option<NodePredicate> {
        self.0.children().find_map(NodePredicate::cast)
    }
}

impl NodePredicate {
    pub fn operator_token(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| predicate_op_from_syntax_kind(k).is_some())
    }

    pub fn operator(&self) -> Option<PredicateOperator> {
        self.operator_token()
            .and_then(|t| predicate_op_from_syntax_kind(t.kind()))
    }

    pub fn string_value(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::StringContent)
    }

    pub fn regex(&self) -> Option<RegexLiteral> {
        self.0.children().find_map(RegexLiteral::cast)
    }
}

impl RegexLiteral {
    /// Returns the regex pattern content (between the `/` delimiters).
    pub fn pattern<'q>(&self, source: &'q str) -> &'q str {
        let range = self.0.text_range();
        let text = &source[usize::from(range.start())..usize::from(range.end())];

        let Some(without_prefix) = text.strip_prefix('/') else {
            return text;
        };
        without_prefix.strip_suffix('/').unwrap_or(without_prefix)
    }
}

impl DefRef {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }
}

impl AlternationPattern {
    pub fn alternatives(&self) -> impl Iterator<Item = Alternative> + '_ {
        self.0.children().filter_map(Alternative::cast)
    }

    pub fn patterns(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.alternatives()
            .filter_map(|alternative| alternative.body())
    }

    pub fn labeling(&self) -> Labeling {
        classify_labeling(&self.0)
    }
}

impl Alternative {
    pub fn label(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }
}

macro_rules! sequence_accessors {
    ($($ty:ident),+ $(,)?) => {
        $(
            impl $ty {
                pub fn children(&self) -> impl Iterator<Item = Pattern> + '_ {
                    self.0.children().filter_map(Pattern::cast)
                }

                /// Returns children interleaved with anchors, preserving order.
                pub fn items(&self) -> impl Iterator<Item = SeqItem> + '_ {
                    self.0.children().filter_map(SeqItem::cast)
                }
            }
        )+
    };
}

sequence_accessors!(NodePattern, SeqPattern);

impl CapturedPattern {
    /// Returns the capture token (@name or @_name).
    /// The token text includes the @ prefix.
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| {
            matches!(k, SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture)
        })
    }

    /// Returns true if this is a suppressive capture (@_ or @_name).
    /// Suppressive captures match structurally but don't contribute to output.
    pub fn is_suppressive(&self) -> bool {
        find_token(&self.0, |k| k == SyntaxKind::SuppressiveCapture).is_some()
    }

    pub fn inner(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }

    pub fn capture_type(&self) -> Option<CaptureTypeSyntax> {
        self.0.children().find_map(CaptureTypeSyntax::cast)
    }
}

impl CaptureTypeSyntax {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }
}

impl QuantifiedPattern {
    pub fn inner(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| {
            matches!(
                k,
                SyntaxKind::Star
                    | SyntaxKind::Plus
                    | SyntaxKind::Question
                    | SyntaxKind::StarQuestion
                    | SyntaxKind::PlusQuestion
                    | SyntaxKind::QuestionQuestion
            )
        })
    }

    /// Classify the quantifier operator into its arity. `None` only for a
    /// malformed quantifier with no operator — the parser guarantees a valid
    /// `QuantifiedPattern` carries one.
    pub fn quantifier_kind(&self) -> Option<QuantifierKind> {
        self.quantifier_operator().map(QuantifierOperator::kind)
    }

    /// Classify the quantifier operator into arity plus greediness. `None` only
    /// for a malformed quantifier with no operator.
    pub fn quantifier_operator(&self) -> Option<QuantifierOperator> {
        quantifier_operator_from_syntax_kind(self.operator()?.kind())
    }

    /// Whether the quantifier repeats (`*`/`+`, greedy or not) — i.e. collects an
    /// array, as opposed to `?`. Reads [`quantifier_kind`](Self::quantifier_kind)
    /// rather than re-listing operators so the lazy twins stay included (#469).
    pub fn is_repeating(&self) -> bool {
        matches!(
            self.quantifier_kind(),
            Some(QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore)
        )
    }

    /// Returns true if quantifier allows zero matches (?, *, ??, *?).
    pub fn is_optional(&self) -> bool {
        matches!(
            self.quantifier_kind(),
            Some(QuantifierKind::Optional | QuantifierKind::ZeroOrMore)
        )
    }
}

impl FieldPattern {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn value(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }
}

impl NegatedField {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }
}

/// Checks if pattern is an empty group (sequence/alternation with no children).
/// Used to distinguish `{ } @x` (empty struct) from `{(pattern) @_} @x` (Node capture).
pub fn is_empty_group(inner: &Pattern) -> bool {
    match inner {
        Pattern::SeqPattern(seq) => seq.children().next().is_none(),
        Pattern::Alternation(a) => a.alternatives().next().is_none(),
        Pattern::QuantifiedPattern(q) => q.inner().is_some_and(|i| is_empty_group(&i)),
        _ => false,
    }
}
