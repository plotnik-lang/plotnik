//! Typed AST wrappers over CST nodes.
//!
//! Each struct wraps a `SyntaxNode` and provides typed accessors.
//! Cast is infallible for correct `SyntaxKind` - validation happens elsewhere.

use crate::ql::syntax_kind::{SyntaxKind, SyntaxNode, SyntaxToken};
use std::fmt::Write;

macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                (node.kind() == SyntaxKind::$kind).then(|| Self(node))
            }

            pub fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(Root, Root);
ast_node!(Def, Def);
ast_node!(Tree, Tree);
ast_node!(Ref, Ref);
ast_node!(Lit, Lit);
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

/// Expression: any pattern that can appear in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Tree(Tree),
    Ref(Ref),
    Lit(Lit),
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
            SyntaxKind::Lit => Lit::cast(node).map(Expr::Lit),
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

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Expr::Tree(n) => n.syntax(),
            Expr::Ref(n) => n.syntax(),
            Expr::Lit(n) => n.syntax(),
            Expr::Str(n) => n.syntax(),
            Expr::Alt(n) => n.syntax(),
            Expr::Seq(n) => n.syntax(),
            Expr::Capture(n) => n.syntax(),
            Expr::Quantifier(n) => n.syntax(),
            Expr::Field(n) => n.syntax(),
            Expr::NegatedField(n) => n.syntax(),
            Expr::Wildcard(n) => n.syntax(),
            Expr::Anchor(n) => n.syntax(),
        }
    }
}

// --- Accessors ---

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

impl Lit {
    pub fn value(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::StrVal)
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

pub fn format_ast(root: &Root) -> String {
    let mut out = String::new();
    format_root(root, &mut out);
    out
}

fn format_root(root: &Root, out: &mut String) {
    out.push_str("Root\n");
    for def in root.defs() {
        format_def(&def, 1, out);
    }
    for expr in root.exprs() {
        format_expr(&expr, 1, out);
    }
}

fn format_def(def: &Def, indent: usize, out: &mut String) {
    let prefix = "  ".repeat(indent);
    let name = def.name().map(|t| t.text().to_string());
    match name {
        Some(n) => {
            let _ = writeln!(out, "{}Def {}", prefix, n);
        }
        None => {
            let _ = writeln!(out, "{}Def", prefix);
        }
    }
    if let Some(body) = def.body() {
        format_expr(&body, indent + 1, out);
    }
}

fn format_expr(expr: &Expr, indent: usize, out: &mut String) {
    let prefix = "  ".repeat(indent);
    match expr {
        Expr::Tree(t) => {
            let node_type = t.node_type().map(|tok| tok.text().to_string());
            match node_type {
                Some(ty) => {
                    let _ = writeln!(out, "{}Tree {}", prefix, ty);
                }
                None => {
                    let _ = writeln!(out, "{}Tree", prefix);
                }
            }
            for child in t.children() {
                format_expr(&child, indent + 1, out);
            }
        }
        Expr::Ref(r) => {
            let name = r.name().map(|t| t.text().to_string()).unwrap_or_default();
            let _ = writeln!(out, "{}Ref {}", prefix, name);
        }
        Expr::Lit(l) => {
            let value = l.value().map(|t| t.text().to_string()).unwrap_or_default();
            let _ = writeln!(out, "{}Lit {}", prefix, value);
        }
        Expr::Str(s) => {
            let value = s.value().map(|t| t.text().to_string()).unwrap_or_default();
            let _ = writeln!(out, "{}Str \"{}\"", prefix, value);
        }
        Expr::Alt(a) => {
            let _ = writeln!(out, "{}Alt", prefix);
            for branch in a.branches() {
                format_branch(&branch, indent + 1, out);
            }
            for expr in a.exprs() {
                format_expr(&expr, indent + 1, out);
            }
        }
        Expr::Seq(s) => {
            let _ = writeln!(out, "{}Seq", prefix);
            for child in s.children() {
                format_expr(&child, indent + 1, out);
            }
        }
        Expr::Capture(c) => {
            let name = c.name().map(|t| t.text().to_string()).unwrap_or_default();
            let type_ann = c
                .type_annotation()
                .and_then(|t| t.name())
                .map(|t| t.text().to_string());
            match type_ann {
                Some(ty) => {
                    let _ = writeln!(out, "{}Capture @{} :: {}", prefix, name, ty);
                }
                None => {
                    let _ = writeln!(out, "{}Capture @{}", prefix, name);
                }
            }
            if let Some(inner) = c.inner() {
                format_expr(&inner, indent + 1, out);
            }
        }
        Expr::Quantifier(q) => {
            let op = q
                .operator()
                .map(|t| t.text().to_string())
                .unwrap_or_default();
            let _ = writeln!(out, "{}Quantifier {}", prefix, op);
            if let Some(inner) = q.inner() {
                format_expr(&inner, indent + 1, out);
            }
        }
        Expr::Field(f) => {
            let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
            let _ = writeln!(out, "{}Field {}:", prefix, name);
            if let Some(value) = f.value() {
                format_expr(&value, indent + 1, out);
            }
        }
        Expr::NegatedField(f) => {
            let name = f.name().map(|t| t.text().to_string()).unwrap_or_default();
            let _ = writeln!(out, "{}NegatedField !{}", prefix, name);
        }
        Expr::Wildcard(_) => {
            let _ = writeln!(out, "{}Wildcard", prefix);
        }
        Expr::Anchor(_) => {
            let _ = writeln!(out, "{}Anchor", prefix);
        }
    }
}

fn format_branch(branch: &Branch, indent: usize, out: &mut String) {
    let prefix = "  ".repeat(indent);
    let label = branch.label().map(|t| t.text().to_string());
    match label {
        Some(l) => {
            let _ = writeln!(out, "{}Branch {}:", prefix, l);
        }
        None => {
            let _ = writeln!(out, "{}Branch", prefix);
        }
    }
    if let Some(body) = branch.body() {
        format_expr(&body, indent + 1, out);
    }
}
