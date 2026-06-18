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

impl Anchor {
    pub fn is_strict(&self) -> bool {
        find_token(&self.0, |k| k == SyntaxKind::DotBang).is_some()
    }
}

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

ast_node!(
    /// Anonymous node: string literal (`"+"`) or wildcard (`_`).
    /// Maps from CST `Str` or `Wildcard`.
    AnonymousNode,
    Str | Wildcard
);

impl AnonymousNode {
    /// Returns the string value if this is a literal, `None` if wildcard.
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
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl NamedNode {
    pub fn node_type(&self) -> Option<SyntaxToken> {
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
        self.node_type()
            .map(|t| t.kind() == SyntaxKind::Underscore)
            .unwrap_or(false)
    }

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
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .skip_while(|t| t.kind() != SyntaxKind::KwMissing)
            .find(|t| matches!(t.kind(), SyntaxKind::Id | SyntaxKind::StrVal))
    }

    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
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

impl AltExpr {
    pub fn kind(&self) -> AltKind {
        let mut tagged = false;
        let mut untagged = false;

        for child in self.0.children().filter(|c| c.kind() == SyntaxKind::Branch) {
            let has_label = find_token(&child, |k| k == SyntaxKind::Id).is_some();

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
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn body(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl SeqExpr {
    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }

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
        find_token(&self.0, |k| {
            matches!(k, SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture)
        })
    }

    /// Returns true if this is a suppressive capture (@_ or @_name).
    /// Suppressive captures match structurally but don't contribute to output.
    pub fn is_suppressive(&self) -> bool {
        find_token(&self.0, |k| k == SyntaxKind::SuppressiveCapture).is_some()
    }

    pub fn inner(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
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

impl QuantifiedExpr {
    pub fn inner(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
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

impl FieldExpr {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
    }

    pub fn value(&self) -> Option<Expr> {
        self.0.children().find_map(Expr::cast)
    }
}

impl NegatedField {
    pub fn name(&self) -> Option<SyntaxToken> {
        find_token(&self.0, |k| k == SyntaxKind::Id)
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
