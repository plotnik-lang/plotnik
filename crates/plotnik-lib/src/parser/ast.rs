//! Typed AST wrappers over CST nodes.
//!
//! Each struct wraps a `SyntaxNode` and provides typed accessors.
//! Cast is infallible for correct `SyntaxKind` - validation happens elsewhere.

use super::cst::{SyntaxKind, SyntaxNode, SyntaxToken};

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
        }
    };
}

ast_node!(Root, Root);
ast_node!(Def, Def);
ast_node!(Tree, Tree);
ast_node!(Ref, Ref);
ast_node!(Str, Str);
ast_node!(Alt, Alt);
ast_node!(Branch, Branch);
ast_node!(Seq, Seq);
ast_node!(Capture, Capture);
ast_node!(Type, Type);
ast_node!(Quantifier, Quantifier);
ast_node!(Field, Field);
ast_node!(NegatedField, NegatedField);
ast_node!(Wildcard, Wildcard);
ast_node!(Anchor, Anchor);

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

/// Expression: any pattern that can appear in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Tree(Tree),
    Ref(Ref),
    Str(Str),
    Alt(Alt),
    Seq(Seq),
    Capture(Capture),
    Quantifier(Quantifier),
    Field(Field),
    NegatedField(NegatedField),
    Wildcard(Wildcard),
    Anchor(Anchor),
}

impl Expr {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::Tree => Tree::cast(node).map(Expr::Tree),
            SyntaxKind::Ref => Ref::cast(node).map(Expr::Ref),
            SyntaxKind::Str => Str::cast(node).map(Expr::Str),
            SyntaxKind::Alt => Alt::cast(node).map(Expr::Alt),
            SyntaxKind::Seq => Seq::cast(node).map(Expr::Seq),
            SyntaxKind::Capture => Capture::cast(node).map(Expr::Capture),
            SyntaxKind::Quantifier => Quantifier::cast(node).map(Expr::Quantifier),
            SyntaxKind::Field => Field::cast(node).map(Expr::Field),
            SyntaxKind::NegatedField => NegatedField::cast(node).map(Expr::NegatedField),
            SyntaxKind::Wildcard => Wildcard::cast(node).map(Expr::Wildcard),
            SyntaxKind::Anchor => Anchor::cast(node).map(Expr::Anchor),
            _ => None,
        }
    }

    pub fn as_cst(&self) -> &SyntaxNode {
        match self {
            Expr::Tree(n) => n.as_cst(),
            Expr::Ref(n) => n.as_cst(),
            Expr::Str(n) => n.as_cst(),
            Expr::Alt(n) => n.as_cst(),
            Expr::Seq(n) => n.as_cst(),
            Expr::Capture(n) => n.as_cst(),
            Expr::Quantifier(n) => n.as_cst(),
            Expr::Field(n) => n.as_cst(),
            Expr::NegatedField(n) => n.as_cst(),
            Expr::Wildcard(n) => n.as_cst(),
            Expr::Anchor(n) => n.as_cst(),
        }
    }
}

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

impl Tree {
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

impl Alt {
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

impl Seq {
    pub fn children(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0.children().filter_map(Expr::cast)
    }
}

impl Capture {
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

impl Quantifier {
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
}

impl Field {
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

impl Str {
    pub fn value(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::StrVal)
    }
}
