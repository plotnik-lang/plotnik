#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[path = "../../plotnik-compiler/src/source/mod.rs"]
pub mod source;

pub use plotnik_core::{DefId, Interner, NodeFieldId, NodeKind, NodeKindId, Symbol, TypeId};
pub use source::{Source, SourceId, SourceKind, SourceMap};
