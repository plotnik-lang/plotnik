//! Abstract syntax tree: parsing, syntax types, and typed node accessors.

pub mod lexer;
pub mod nodes;
pub mod parser;
pub mod syntax_kind;

#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod nodes_tests;

pub use syntax_kind::{SyntaxKind, SyntaxNode, SyntaxToken};

pub use nodes::{
    Alt, AltKind, Anchor, Branch, Capture, Def, Expr, Field, Lit, NegatedField, Quantifier, Ref,
    Root, Seq, Str, Tree, Type, Wildcard,
};

pub use parser::{Parse, parse};

pub use parser::{ErrorStage, Fix, RelatedInfo, SyntaxError, render_errors};

pub use nodes::format_ast;
