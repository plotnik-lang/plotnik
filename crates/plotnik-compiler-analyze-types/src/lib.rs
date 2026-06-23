#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}

pub mod parser {
    pub use plotnik_compiler_core::ast;
    pub use plotnik_compiler_core::{
        AltKind, Anchor, Branch, CapturedPattern, Def, EnumPattern, FieldPattern, NegatedField,
        NodePattern, NodePredicate, Pattern, PredicateOp, PredicateValue, QuantifiedPattern, Ref,
        RegexLiteral, Root, SeqItem, SeqPattern, SyntaxKind, SyntaxNode, SyntaxToken, TokenPattern,
        Type, UnionPattern, classify_alt, is_empty_group, token_src,
    };
}

pub mod source {
    pub use plotnik_compiler_diagnostics::source::*;
}

pub use plotnik_compiler_diagnostics::{Diagnostics, SourceId};

pub mod analyze;
pub use analyze::*;
