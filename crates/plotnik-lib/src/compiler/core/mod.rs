#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod ast;
pub mod capture_mechanism;
pub mod cst;
pub mod dependency_analysis;
pub mod emit;
pub mod grammar_binding;
pub mod ir;
pub mod located;
pub mod source;
pub mod span;
pub mod symbol_table;
pub mod type_analysis;
pub mod type_shape;
pub mod validated_ast;
pub mod visitor;

#[cfg(test)]
mod cst_tests;
#[cfg(test)]
mod ir_tests;

pub use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId, Symbol};
pub use ast::{
    AltKind, Anchor, Branch, CapturedPattern, Def, EnumPattern, FieldPattern, NegatedField,
    NodePattern, NodePredicate, Pattern, PredicateOp, PredicateValue, QuantifiedPattern, Ref,
    RegexLiteral, Root, SeqItem, SeqPattern, TokenPattern, Type, UnionPattern, classify_alt,
    is_empty_group, token_src,
};
pub use capture_mechanism::CaptureMechanism;
pub use cst::{QueryLang, SyntaxKind, SyntaxNode, SyntaxToken};
pub use dependency_analysis::DependencyAnalysis;
pub use emit::{
    EASTER_EGG, EmitError, EmitInput, RegexTableBuilder, StringTableBuilder, TypeTableBuilder,
};
pub use grammar_binding::GrammarBinding;
pub use located::Located;
pub use source::{Source, SourceId, SourceKind, SourceMap};
pub use span::Span;
pub use symbol_table::SymbolTable;
pub use type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
pub use type_shape::{FieldInfo, OutputFlow, PatternResult, TypeShape};
pub use validated_ast::ValidatedAst;

/// A lightweight handle to a named query definition.
///
/// Assigned during dependency analysis and shared by later compiler artifacts.
/// Ordered by assignment index, which is SCC processing order (leaves first):
/// iterating a `DefId`-keyed map yields definitions in emission order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DefId(u32);

impl DefId {
    #[inline]
    pub fn from_raw(index: u32) -> Self {
        Self(index)
    }

    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Interned query type identifier.
///
/// Indexes the analysis-time type registry. This is distinct from the serialized
/// bytecode `TypeId`, which is compacted during emission.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeId(pub u32);

impl TypeId {
    #[inline]
    pub fn is_builtin(self) -> bool {
        self.0 <= 1
    }
}
