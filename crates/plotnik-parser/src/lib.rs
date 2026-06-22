#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics {
    pub use plotnik_diagnostics::diagnostics::*;
}

pub mod source {
    pub use plotnik_diagnostics::source::*;
}

pub use plotnik_diagnostics::{Error, Result};

pub mod parser {
    #[path = "../../../plotnik-compiler/src/parser/ast.rs"]
    pub mod ast;
    #[path = "../../../plotnik-compiler/src/parser/cst.rs"]
    mod cst;
    #[path = "../../../plotnik-compiler/src/parser/lexer.rs"]
    mod lexer;

    #[path = "../../../plotnik-compiler/src/parser/core.rs"]
    mod core;
    #[path = "../../../plotnik-compiler/src/parser/grammar/mod.rs"]
    mod grammar;
    #[path = "../../../plotnik-compiler/src/parser/invariants.rs"]
    mod invariants;

    pub use ast::{
        AltKind, Anchor, Branch, CapturedPattern, Def, EnumPattern, FieldPattern, NegatedField,
        NodePattern, NodePredicate, Pattern, PredicateOp, PredicateValue, QuantifiedPattern, Ref,
        RegexLiteral, Root, SeqItem, SeqPattern, TokenPattern, Type, UnionPattern, is_empty_group,
        token_src,
    };
    pub use ast::classify_alt;
    pub use core::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, ParseResult, Parser};
    pub use cst::{SyntaxKind, SyntaxNode, SyntaxToken};
    pub use lexer::{Token, dump_tokens, lex, token_text};
}

pub use parser::*;
