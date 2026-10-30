use std::collections::HashMap;

use rowan::TextRange;

use super::{DefinitionGraph, validate_source_cycles};
use crate::compiler::diagnostics::report::{Diagnostics, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::core::Interner;

fn empty_definitions() -> DefinitionGraph {
    DefinitionGraph::new(
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        HashMap::new(),
    )
}

fn use_span(source: u32) -> Span {
    Span::new(SourceId(source), TextRange::new(0.into(), 1.into()))
}

#[test]
fn ownership_edges_close_a_source_cycle() {
    let definitions = empty_definitions();
    let interner = Interner::new();
    let mut diag = Diagnostics::new();
    let uses = [(use_span(0), SourceId(1)), (use_span(1), SourceId(0))];

    let valid = validate_source_cycles(&definitions, &interner, &uses, &mut diag);

    assert!(!valid);
    assert_eq!(diag.error_count(), 1);
}

#[test]
fn same_source_ownership_is_not_a_cycle() {
    let definitions = empty_definitions();
    let interner = Interner::new();
    let mut diag = Diagnostics::new();
    let uses = [(use_span(0), SourceId(0))];

    let valid = validate_source_cycles(&definitions, &interner, &uses, &mut diag);

    assert!(valid);
    assert!(diag.is_empty());
}

#[test]
fn diamond_ownership_is_not_a_cycle() {
    let definitions = empty_definitions();
    let interner = Interner::new();
    let mut diag = Diagnostics::new();
    let uses = [
        (use_span(0), SourceId(1)),
        (use_span(0), SourceId(2)),
        (use_span(1), SourceId(3)),
        (use_span(2), SourceId(3)),
    ];

    let valid = validate_source_cycles(&definitions, &interner, &uses, &mut diag);

    assert!(valid);
    assert!(diag.is_empty());
}
