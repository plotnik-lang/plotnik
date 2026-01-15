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

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then(|| Self(node))
            }

            pub fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            pub fn as_cst(&self) -> &SyntaxNode {
                &self.0
            }

            pub fn text_range(&self) -> TextRange {
                self.0.text_range()
            }
        }
    };
}

macro_rules! define_expr {
    ($($variant:ident),+ $(,)?) => {
        /// Expression: any pattern that can appear in the tree.
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum Expr {
            $($variant($variant)),+
        }

        impl Expr {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                let kind = node.kind();
                $(if $variant::can_cast(kind) { return Some(Expr::$variant($variant(node))); })+
                None
            }

            pub fn as_cst(&self) -> &SyntaxNode {
                match self { $(Expr::$variant(n) => n.as_cst()),+ }
            }

            pub fn text_range(&self) -> TextRange {
                match self { $(Expr::$variant(n) => n.text_range()),+ }
            }
        }
    };
}

impl Expr {
    /// Returns direct child expressions.
    pub fn children(&self) -> Vec<Expr> {
        match self {
            Expr::NamedNode(n) => n.children().collect(),
            Expr::SeqExpr(s) => s.children().collect(),
            Expr::CapturedExpr(c) => c.inner().into_iter().collect(),
            Expr::QuantifiedExpr(q) => q.inner().into_iter().collect(),
            Expr::FieldExpr(f) => f.value().into_iter().collect(),
            Expr::AltExpr(a) => a.branches().filter_map(|b| b.body()).collect(),
            Expr::Ref(_) | Expr::AnonymousNode(_) => vec![],
        }
    }
}

ast_node!(Root, Root);
ast_node!(Def, Def);
ast_node!(NamedNode, Tree);
ast_node!(Ref, Ref);
ast_node!(AltExpr, Alt);
ast_node!(Branch, Branch);
ast_node!(SeqExpr, Seq);
ast_node!(CapturedExpr, Capture);
ast_node!(Type, Type);
ast_node!(QuantifiedExpr, Quantifier);
ast_node!(FieldExpr, Field);
ast_node!(NegatedField, NegatedField);
ast_node!(Anchor, Anchor);
ast_node!(NodePredicate, NodePredicate);
ast_node!(RegexLiteral, Regex);

/// Either an expression or an anchor in a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SeqItem {
    Expr(Expr),
    Anchor(Anchor),
}

impl SeqItem {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        if let Some(expr) = Expr::cast(node.clone()) {
            return Some(SeqItem::Expr(expr));
        }
        if let Some(anchor) = Anchor::cast(node) {
            return Some(SeqItem::Anchor(anchor));
        }
        None
    }

    pub fn as_anchor(&self) -> Option<&Anchor> {
        match self {
            SeqItem::Anchor(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_expr(&self) -> Option<&Expr> {
        match self {
            SeqItem::Expr(e) => Some(e),
            _ => None,
        }
    }
}

/// Anonymous node: string literal (`"+"`) or wildcard (`_`).
/// Maps from CST `Str` or `Wildcard`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnonymousNode(SyntaxNode);

impl AnonymousNode {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| Self(node))
    }

    pub fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::Str | SyntaxKind::Wildcard)
    }

    pub fn as_cst(&self) -> &SyntaxNode {
        &self.0
    }

    pub fn text_range(&self) -> TextRange {
        self.0.text_range()
    }

    /// Returns the string value if this is a literal, `None` if wildcard.
    pub fn value(&self) -> Option<SyntaxToken> {
        if self.0.kind() == SyntaxKind::Wildcard {
            return None;
        }
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::StrVal)
    }

    /// Returns true if this is the "any" wildcard (`_`).
    pub fn is_any(&self) -> bool {
        self.0.kind() == SyntaxKind::Wildcard
    }
}

/// Whether an alternation uses tagged or untagged branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AltKind {
    /// All branches have labels: `[A: expr1 B: expr2]`
    Tagged,
    /// No branches have labels: `[expr1 expr2]`
    Untagged,
    /// Mixed tagged and untagged branches (invalid)
    Mixed,
}

/// Predicate operator for node text filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredicateOp {
    /// `==` - equals
    Eq,
    /// `!=` - not equals
    Ne,
    /// `^=` - starts with
    StartsWith,
    /// `$=` - ends with
    EndsWith,
    /// `*=` - contains
    Contains,
    /// `=~` - regex match
    RegexMatch,
    /// `!~` - regex no match
    RegexNoMatch,
}

impl PredicateOp {
    pub fn from_syntax_kind(kind: SyntaxKind) -> Option<Self> {
        match kind {
            SyntaxKind::OpEq => Some(Self::Eq),
            SyntaxKind::OpNe => Some(Self::Ne),
            SyntaxKind::OpStartsWith => Some(Self::StartsWith),
            SyntaxKind::OpEndsWith => Some(Self::EndsWith),
            SyntaxKind::OpContains => Some(Self::Contains),
            SyntaxKind::OpRegexMatch => Some(Self::RegexMatch),
            SyntaxKind::OpRegexNoMatch => Some(Self::RegexNoMatch),
            _ => None,
        }
    }

    /// Decode from bytecode representation.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::StartsWith,
            3 => Self::EndsWith,
            4 => Self::Contains,
            5 => Self::RegexMatch,
            6 => Self::RegexNoMatch,
            _ => panic!("invalid predicate op byte: {b}"),
        }
    }

    /// Encode for bytecode.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Eq => 0,
            Self::Ne => 1,
            Self::StartsWith => 2,
            Self::EndsWith => 3,
            Self::Contains => 4,
            Self::RegexMatch => 5,
            Self::RegexNoMatch => 6,
        }
    }

    /// Operator as display string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::StartsWith => "^=",
            Self::EndsWith => "$=",
            Self::Contains => "*=",
            Self::RegexMatch => "=~",
            Self::RegexNoMatch => "!~",
        }
    }

    pub fn is_regex_op(&self) -> bool {
        matches!(self, Self::RegexMatch | Self::RegexNoMatch)
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

define_expr!(
    NamedNode,
    Ref,
    AnonymousNode,
    AltExpr,
    SeqExpr,
    CapturedExpr,
    QuantifiedExpr,
    FieldExpr,
);

impl Root {
    pub fn defs(&self) -> impl Iterator<Item = Def> + '_ {
        self.0.children().filter_map(Def::cast)
    }

    pub fn exprs(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }
}

impl Def {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl NamedNode {
    pub fn node_type(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| {
                matches!(
                    t.kind(),
                    SyntaxKind::Id
                        | SyntaxKind::Underscore
                        | SyntaxKind::KwError
                        | SyntaxKind::KwMissing
                )
            })
    }

    /// Returns true if the node type is wildcard (`_`), matching any named node.
    pub fn is_any(&self) -> bool {
        self.node_type()
            .map(|t| t.kind() == SyntaxKind::Underscore)
            .unwrap_or(false)
    }

    /// Returns true if this is a MISSING node: `(MISSING ...)`.
    pub fn is_missing(&self) -> bool {
        self.node_type()
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
        // After KwMissing, look for Id or StrVal token
        let mut found_missing = false;
        for child in self.0.children_with_tokens() {
            if let Some(token) = child.into_token() {
                if token.kind() == SyntaxKind::KwMissing {
                    found_missing = true;
                } else if found_missing
                    && matches!(token.kind(), SyntaxKind::Id | SyntaxKind::StrVal)
                {
                    return Some(token);
                }
            }
        }
        None
    }

    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }

    /// Returns all anchors in this node.
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
    /// Returns the operator token.
    pub fn operator_token(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| PredicateOp::from_syntax_kind(t.kind()).is_some())
    }

    /// Returns the operator kind.
    pub fn operator(&self) -> Option<PredicateOp> {
        self.operator_token()
            .and_then(|t| PredicateOp::from_syntax_kind(t.kind()))
    }

    /// Returns the string value if the predicate uses a string.
    pub fn string_value(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::StrVal)
    }

    /// Returns the regex literal if the predicate uses a regex.
    pub fn regex(&self) -> Option<RegexLiteral> {
        self.0.children().find_map(RegexLiteral::cast)
    }

    /// Returns the predicate value (string or regex pattern).
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
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }
}

impl AltExpr {
    pub fn kind(&self) -> AltKind {
        let mut tagged = false;
        let mut untagged = false;

        for child in self.0.children().filter(|c| c.kind() == SyntaxKind::Branch) {
            let has_label = child
                .children_with_tokens()
                .filter_map(|it| it.into_token())
                .any(|t| t.kind() == SyntaxKind::Id);

            if has_label {
                tagged = true;
            } else {
                untagged = true;
            }
        }

        match (tagged, untagged) {
            (true, true) => AltKind::Mixed,
            (true, false) => AltKind::Tagged,
            _ => AltKind::Untagged,
        }
    }

    pub fn branches(&self) -> impl Iterator<Item = Branch> + '_ {
        self.0.children().filter_map(Branch::cast)
    }

    pub fn exprs(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }
}

impl Branch {
    pub fn label(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl SeqExpr {
    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }

    /// Returns all anchors in this sequence.
    pub fn anchors(&self) -> impl Iterator<Item = Anchor> + '_ {
        self.0.children().filter_map(Anchor::cast)
    }

    /// Returns children interleaved with anchors, preserving order.
    pub fn items(&self) -> impl Iterator<Item = SeqItem> + '_ {
        self.0.children().filter_map(SeqItem::cast)
    }
}

impl CapturedExpr {
    /// Returns the capture token (@name or @_name).
    /// The token text includes the @ prefix.
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| {
                matches!(
                    t.kind(),
                    SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture
                )
            })
    }

    /// Returns true if this is a suppressive capture (@_ or @_name).
    /// Suppressive captures match structurally but don't contribute to output.
    pub fn is_suppressive(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|t| t.kind() == SyntaxKind::SuppressiveCapture)
    }

    pub fn inner(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }

    pub fn type_annotation(&self) -> Option<Type> {
        self.0.children().find_map(Type::cast)
    }

    /// Returns true if this capture has a `:: string` type annotation.
    pub fn has_string_annotation(&self) -> bool {
        self.type_annotation()
            .is_some_and(|t| t.name().is_some_and(|n| n.text() == "string"))
    }
}

impl Type {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }
}

impl QuantifiedExpr {
    pub fn inner(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| {
                matches!(
                    t.kind(),
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

impl FieldExpr {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }

    pub fn value(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl NegatedField {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
    }
}

/// Checks if expression is a truly empty scope (sequence/alternation with no children).
/// Used to distinguish `{ } @x` (empty struct) from `{(expr) @_} @x` (Node capture).
pub fn is_truly_empty_scope(inner: &Expr) -> bool {
    match inner {
        Expr::SeqExpr(seq) => seq.children().next().is_none(),
        Expr::AltExpr(alt) => alt.branches().next().is_none(),
        Expr::QuantifiedExpr(q) => q.inner().is_some_and(|i| is_truly_empty_scope(&i)),
        _ => false,
    }
}
