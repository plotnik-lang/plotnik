//! Typed AST wrappers over CST nodes.
//!
//! Each struct wraps a `SyntaxNode` and provides typed accessors.
//! Cast is infallible for correct `SyntaxKind` - validation happens elsewhere.
//!
//! ## String Lifetime Limitation
//!
//! `SyntaxToken::text()` returns `&str` tied to the token's lifetime, not to the
//! source `&'src str`. This is a rowan design: tokens store interned strings, not
//! spans into the original source.
//!
//! When building data structures that need source-lifetime strings (e.g.,
//! `SymbolTable<'src>`), use [`token_src`] instead of `token.text()`.

use super::cst::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::TextRange;

/// Extracts token text with source lifetime.
///
/// Use this instead of `token.text()` when you need `&'src str`.
pub fn token_src<'src>(token: &SyntaxToken, source: &'src str) -> &'src str {
    let range = token.text_range();
    &source[range.start().into()..range.end().into()]
}

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                (node.kind() == SyntaxKind::$kind).then(|| Self(node))
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
                $(if let Some(n) = $variant::cast(node.clone()) { return Some(Expr::$variant(n)); })+
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

/// Anonymous node: string literal (`"+"`) or wildcard (`_`).
/// Maps from CST `Str` or `Wildcard`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnonymousNode(SyntaxNode);

impl AnonymousNode {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        matches!(node.kind(), SyntaxKind::Str | SyntaxKind::Wildcard).then(|| Self(node))
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

    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
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
}

impl CapturedExpr {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::Id)
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
