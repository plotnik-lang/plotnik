use std::fmt::Write as _;

use crate::bytecode::{HEADER_SIZE, Header, SPAN_NO_BINDING, STEP_SIZE, SpanEntry, SpanKind};
use crate::compiler::diagnostics::DiagnosticKind;
use crate::compiler::test_utils::javascript_grammar;
use crate::compiler::{CompiledQuery, QueryBuilder};

#[test]
fn assigns_dense_spans_in_walk_order() {
    let src = exact_range_query();
    let spans = inspected_spans(src);

    let kinds: Vec<_> = spans.iter().map(|span| span.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SpanKind::Def,
            SpanKind::Pattern,
            SpanKind::Quantifier,
            SpanKind::Pattern,
            SpanKind::Pattern,
            SpanKind::Pattern,
            SpanKind::Capture,
            SpanKind::Annotation,
            SpanKind::Field,
            SpanKind::Pattern,
        ]
    );
    assert!(spans.iter().all(|span| span.source == 0));
}

#[test]
fn token_spans_cover_capture_quantifier_field_and_annotation_tokens() {
    let src = exact_range_query();
    let spans = inspected_spans(src);

    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Capture)), "@id");
    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Quantifier)), "?");
    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Field)), "name");

    let annotation = find_kind(&spans, SpanKind::Annotation);
    assert_eq!(span_text(src, annotation), ":: Ident");
    assert_ne!(annotation.type_id, SPAN_NO_BINDING);
    assert_eq!(annotation.member, SPAN_NO_BINDING);
}

#[test]
fn ladder_drops_whole_lower_priority_tiers() {
    let mut src = String::new();
    for i in 0..600 {
        writeln!(src, "Q{i} = (identifier) @c_{i}").expect("write query");
    }

    let compiled = inspected(&src);
    assert!(
        compiled
            .diagnostics()
            .kinds()
            .any(|kind| kind == DiagnosticKind::InspectionSpansDegraded),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );

    let spans: Vec<_> = compiled
        .module()
        .expect("valid query should compile")
        .spans()
        .iter()
        .collect();
    assert_eq!(spans.len(), 600);
    assert!(spans.iter().all(|span| span.kind == SpanKind::Def));
}

#[test]
fn inspection_does_not_change_transition_bytes_yet() {
    let src = "Q = (program (expression_statement (identifier) @id))";
    let plain = QueryBuilder::from_inline(src)
        .compile(javascript_grammar())
        .expect("query parsing should not exhaust fuel");
    let inspected = inspected(src);

    assert_eq!(
        transition_bytes(plain.bytecode().expect("plain bytecode")),
        transition_bytes(inspected.bytecode().expect("inspected bytecode")),
    );
    assert!(
        !inspected
            .module()
            .expect("inspected module")
            .spans()
            .is_empty()
    );
}

fn inspected_spans(src: &str) -> Vec<SpanEntry> {
    inspected(src)
        .module()
        .expect("valid query should compile")
        .spans()
        .iter()
        .collect()
}

fn exact_range_query() -> &'static str {
    "Q = (program (expression_statement)? @_ (lexical_declaration (variable_declarator name: (identifier) @id :: Ident)))"
}

fn inspected(src: &str) -> CompiledQuery {
    let compiled = QueryBuilder::from_inline(src)
        .with_inspection(true)
        .compile(javascript_grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled
}

fn find_kind(spans: &[SpanEntry], kind: SpanKind) -> SpanEntry {
    spans
        .iter()
        .copied()
        .find(|span| span.kind == kind)
        .expect("span kind should be present")
}

fn span_text(src: &str, span: SpanEntry) -> &str {
    &src[span.start as usize..span.end as usize]
}

fn transition_bytes(bytes: &[u8]) -> &[u8] {
    let header = Header::from_bytes(&bytes[..HEADER_SIZE]);
    let offsets = header.compute_offsets();
    let start = offsets.transitions as usize;
    let len = header.transitions_count as usize * STEP_SIZE;
    &bytes[start..start + len]
}
