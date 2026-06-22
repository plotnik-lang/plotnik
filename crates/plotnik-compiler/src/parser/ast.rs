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

use super::cst::{SyntaxKind, SyntaxNode, SyntaxToken};
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
            /// Union alternation `[a b]`.
            Union(UnionPattern),
            /// Enum alternation `[A: a B: b]`.
            Enum(EnumPattern),
        }

        impl Pattern {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                let kind = node.kind();
                // `UnionPattern` and `EnumPattern` both wrap `SyntaxKind::Alt`;
                // branch labels decide which. This is the one syntactic boundary
                // where the two concepts still meet — a mixed alternation recovers
                // as a union (its `MixedAltBranches` diagnostic is raised separately).
                if kind == SyntaxKind::Alt {
                    return Some(match classify_alt(&node) {
                        AltKind::Enum => Pattern::Enum(EnumPattern(node)),
                        AltKind::Union | AltKind::Mixed => Pattern::Union(UnionPattern(node)),
                    });
                }
                $(if $variant::can_cast(kind) { return Some(Pattern::$variant($variant(node))); })+
                None
            }

            pub fn syntax(&self) -> &SyntaxNode {
                match self {
                    $(Pattern::$variant(n) => n.syntax(),)+
                    Pattern::Union(n) => n.syntax(),
                    Pattern::Enum(n) => n.syntax(),
                }
            }

            pub fn text_range(&self) -> TextRange {
                match self {
                    $(Pattern::$variant(n) => n.text_range(),)+
                    Pattern::Union(n) => n.text_range(),
                    Pattern::Enum(n) => n.text_range(),
                }
            }
        }
    };
}

impl Pattern {
    pub fn children(&self) -> Vec<Pattern> {
        match self {
            Pattern::NodePattern(n) => n.children().collect(),
            Pattern::SeqPattern(s) => s.children().collect(),
            Pattern::CapturedPattern(c) => c.inner().into_iter().collect(),
            Pattern::QuantifiedPattern(q) => q.inner().into_iter().collect(),
            Pattern::FieldPattern(f) => f.value().into_iter().collect(),
            Pattern::Union(u) => u.branches().filter_map(|b| b.body()).collect(),
            Pattern::Enum(e) => e.branches().filter_map(|b| b.body()).collect(),
            Pattern::Ref(_) | Pattern::TokenPattern(_) => vec![],
        }
    }
}

ast_node!(Root, Root);
ast_node!(Def, Def);
ast_node!(NodePattern, Tree);
ast_node!(Ref, Ref);
// `UnionPattern` and `EnumPattern` both refine `SyntaxKind::Alt`, told apart
// only by their branch labels. A kind-only `cast`/`can_cast` (as `ast_node!`
// generates) could not distinguish them — it would wrap an enum alternation as
// a union and vice versa — so they are defined by hand without one.
// Classification happens exactly once, in `Pattern::cast` (via `classify_alt`),
// which is their sole constructor.

/// Union alternation `[a b]`. A refinement of `SyntaxKind::Alt`,
/// constructed only by `Pattern::cast` when no branch carries a label (mixed
/// alternations recover here too; their `MixedAltBranches` diagnostic is raised
/// separately).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnionPattern(SyntaxNode);

/// Enum alternation `[A: a B: b]`. A refinement of `SyntaxKind::Alt`,
/// constructed only by `Pattern::cast` when every branch carries a label.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumPattern(SyntaxNode);
ast_node!(Branch, Branch);
ast_node!(SeqPattern, Seq);
ast_node!(CapturedPattern, Capture);
ast_node!(Type, Type);
ast_node!(QuantifiedPattern, Quantifier);
ast_node!(FieldPattern, Field);
ast_node!(NegatedField, NegatedField);
ast_node!(Anchor, Anchor);
ast_node!(NodePredicate, NodePredicate);
ast_node!(RegexLiteral, Regex);

impl Anchor {
    pub fn is_strict(&self) -> bool {
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

    pub fn as_anchor(&self) -> Option<&Anchor> {
        match self {
            SeqItem::Anchor(a) => Some(a),
            _ => None,
        }
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
        find_token(&self.0, |k| k == SyntaxKind::StrVal)
    }

    pub fn is_any(&self) -> bool {
        self.0.kind() == SyntaxKind::Wildcard
    }
}

/// Syntactic classification of an alternation `[...]` by its branch labels.
///
/// This is the *only* place union and enum are still one concept. `Pattern::cast`
/// uses it to fork into `UnionPattern`/`EnumPattern` (collapsing `Mixed` to a
/// union for recovery); the `MixedAltBranches` diagnostic uses it to flag the
/// invalid case. Nothing downstream of `Pattern::cast` ever sees an `AltKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AltKind {
    /// All branches have labels: `[A: expr1 B: expr2]`
    Enum,
    /// No branches have labels: `[expr1 expr2]`
    Union,
    /// Mixed enum and union branches (invalid)
    Mixed,
}

/// Classify an alternation node by scanning its branches for labels — the shared
/// boundary used by `Pattern::cast` and the mixed-branch diagnostic.
pub fn classify_alt(node: &SyntaxNode) -> AltKind {
    let mut is_enum = false;
    let mut is_union = false;

    for child in node.children().filter(|c| c.kind() == SyntaxKind::Branch) {
        if find_token(&child, |k| k == SyntaxKind::Id).is_some() {
            is_enum = true;
        } else {
            is_union = true;
        }
    }

    match (is_enum, is_union) {
        (true, true) => AltKind::Mixed,
        (true, false) => AltKind::Enum,
        _ => AltKind::Union,
    }
}

pub use plotnik_bytecode::PredicateOp;

fn predicate_op_from_syntax_kind(kind: SyntaxKind) -> Option<PredicateOp> {
    match kind {
        SyntaxKind::OpEq => Some(PredicateOp::Eq),
        SyntaxKind::OpNe => Some(PredicateOp::Ne),
        SyntaxKind::OpStartsWith => Some(PredicateOp::StartsWith),
        SyntaxKind::OpEndsWith => Some(PredicateOp::EndsWith),
        SyntaxKind::OpContains => Some(PredicateOp::Contains),
        SyntaxKind::OpRegexMatch => Some(PredicateOp::RegexMatch),
        SyntaxKind::OpRegexNoMatch => Some(PredicateOp::RegexNoMatch),
        _ => None,
    }
}

/// Predicate value: either a string or a regex pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateValue<'q> {
    /// String literal value
    String(&'q str),
    /// Regex pattern (the content between `/` delimiters)
    Regex(&'q str),
}

define_pattern!(
    NodePattern,
    Ref,
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

    /// For a `#subtype` (or deprecated `/subtype`) refinement, returns the subtype kind
    /// token: `(expression#statement_block)` → `statement_block`. Returns `None` for a bare
    /// category `(expression#)`, a plain node, or a string subtype (`#"x"`) — the last of
    /// which structural validation conservatively skips.
    pub fn subtype(&self) -> Option<SyntaxToken> {
        let mut tokens = self
            .0
            .children_with_tokens()
            .filter_map(|it| it.into_token());
        tokens
            .by_ref()
            .find(|t| matches!(t.kind(), SyntaxKind::Hash | SyntaxKind::Slash))?;
        tokens.find(|t| t.kind() == SyntaxKind::Id)
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

    /// For MISSING nodes, returns the inner type constraint if present.
    ///
    /// `(MISSING identifier)` → Some("identifier")
    /// `(MISSING ";")` → Some(";")
    /// `(MISSING)` → None
    pub fn missing_constraint(&self) -> Option<SyntaxToken> {
        if !self.is_missing() {
            return None;
        }
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .skip_while(|t| t.kind() != SyntaxKind::KwMissing)
            .find(|t| matches!(t.kind(), SyntaxKind::Id | SyntaxKind::StrVal))
    }

    pub fn children(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.0.children().filter_map(Pattern::cast)
    }

    pub fn anchors(&self) -> impl Iterator<Item = Anchor> + '_ {
        self.0.children().filter_map(Anchor::cast)
    }

    /// Returns children interleaved with anchors, preserving order.
    pub fn items(&self) -> impl Iterator<Item = SeqItem> + '_ {
        self.0.children().filter_map(SeqItem::cast)
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

    pub fn operator(&self) -> Option<PredicateOp> {
        self.operator_token()
            .and_then(|t| predicate_op_from_syntax_kind(t.kind()))
    }

    pub fn string_value(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::StrVal)
    }

    pub fn regex(&self) -> Option<RegexLiteral> {
        self.0.children().find_map(RegexLiteral::cast)
    }

    pub fn value<'q>(&self, source: &'q str) -> Option<PredicateValue<'q>> {
        if let Some(str_token) = self.string_value() {
            return Some(PredicateValue::String(token_src(&str_token, source)));
        }
        if let Some(regex) = self.regex() {
            return Some(PredicateValue::Regex(regex.pattern(source)));
        }
        None
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

impl Ref {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }
}

impl UnionPattern {
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    pub fn text_range(&self) -> TextRange {
        self.0.text_range()
    }

    pub fn branches(&self) -> impl Iterator<Item = Branch> + '_ {
        self.0.children().filter_map(Branch::cast)
    }

    /// Bare (non-`Branch`-wrapped) patterns — only present on a malformed tree;
    /// a well-formed alternation wraps every branch in `SyntaxKind::Branch`.
    pub fn patterns(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.0.children().filter_map(Pattern::cast)
    }
}

impl EnumPattern {
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    pub fn text_range(&self) -> TextRange {
        self.0.text_range()
    }

    pub fn branches(&self) -> impl Iterator<Item = Branch> + '_ {
        self.0.children().filter_map(Branch::cast)
    }

    pub fn patterns(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.0.children().filter_map(Pattern::cast)
    }
}

impl Branch {
    pub fn label(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Pattern> {
        self.0.children().find_map(Pattern::cast)
    }
}

impl SeqPattern {
    pub fn children(&self) -> impl Iterator<Item = Pattern> + '_ {
        self.0.children().filter_map(Pattern::cast)
    }

    pub fn anchors(&self) -> impl Iterator<Item = Anchor> + '_ {
        self.0.children().filter_map(Anchor::cast)
    }

    /// Returns children interleaved with anchors, preserving order.
    pub fn items(&self) -> impl Iterator<Item = SeqItem> + '_ {
        self.0.children().filter_map(SeqItem::cast)
    }
}

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

    pub fn type_annotation(&self) -> Option<Type> {
        self.0.children().find_map(Type::cast)
    }
}

impl Type {
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

    /// Returns true if quantifier allows zero matches (?, *, ??, *?).
    pub fn is_optional(&self) -> bool {
        self.operator()
            .map(|op| {
                matches!(
                    op.kind(),
                    SyntaxKind::Question
                        | SyntaxKind::Star
                        | SyntaxKind::QuestionQuestion
                        | SyntaxKind::StarQuestion
                )
            })
            .unwrap_or(false)
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
        Pattern::Union(u) => u.branches().next().is_none(),
        Pattern::Enum(e) => e.branches().next().is_none(),
        Pattern::QuantifiedPattern(q) => q.inner().is_some_and(|i| is_empty_group(&i)),
        _ => false,
    }
}
