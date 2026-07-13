use std::fmt::Write as _;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, HEADER_SIZE, Header, MAX_SPANS, SPAN_NO_BINDING, SpanEntry, SpanKind,
};
use crate::compiler::diagnostics::DiagnosticKind;
use crate::compiler::test_utils::synthetic_grammar;
use crate::compiler::{BytecodeConfig, BytecodeInspection, CompiledQuery, Emission, QueryBuilder};

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
            SpanKind::CaptureType,
            SpanKind::Field,
            SpanKind::Pattern,
        ]
    );
    assert!(spans.iter().all(|span| span.source == 0));
}

#[test]
fn token_spans_cover_capture_quantifier_field_and_capture_type_tokens() {
    let src = exact_range_query();
    let spans = inspected_spans(src);

    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Capture)), "@id");
    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Quantifier)), "?");
    assert_eq!(span_text(src, find_kind(&spans, SpanKind::Field)), "name");

    let capture_type = find_kind(&spans, SpanKind::CaptureType);
    assert_eq!(span_text(src, capture_type), ":: Ident");
    assert_ne!(capture_type.type_id, SPAN_NO_BINDING);
    assert_eq!(capture_type.member, SPAN_NO_BINDING);
}

#[test]
fn semantic_span_bindings_survive_bytecode_projection() {
    let spans = inspected_spans("Q = (program (expression_statement (identifier) @id))");

    let definition = find_kind(&spans, SpanKind::Def);
    assert_ne!(definition.type_id, SPAN_NO_BINDING);
    assert_eq!(definition.member, SPAN_NO_BINDING);

    let capture = find_kind(&spans, SpanKind::Capture);
    assert_ne!(capture.type_id, SPAN_NO_BINDING);
    assert_ne!(capture.member, SPAN_NO_BINDING);
}

#[test]
fn unused_fragment_spans_are_not_projected() {
    let src = "Unused = (comment)* @unused\n\
               Used = (comment)+ @used\n\
               Q = (program (Used))";
    let unused_end = src
        .find("Used =")
        .expect("query contains the used fragment");
    let spans = inspected_spans(src);

    assert!(
        spans
            .iter()
            .all(|span| usize::try_from(span.start).expect("span offset fits usize") >= unused_end),
        "unused definition contributed an inspection span: {spans:?}"
    );
    assert!(
        spans
            .iter()
            .any(|span| span_text(src, *span).contains("Used")),
        "reachable fragment spans must remain: {spans:?}"
    );
}

#[test]
fn ladder_drops_whole_lower_priority_tiers() {
    // Each definition contributes one definition, capture, and pattern span.
    // One more than a third of the budget admits the first two complete tiers
    // but forces the pattern tier over the module-wide limit.
    let definition_count = MAX_SPANS / 3 + 1;
    let mut src = String::new();
    for i in 0..definition_count {
        writeln!(src, "Q{i} = (identifier) @c_{i}").expect("write query");
    }

    let compiled = compiled(&src);
    let emission = inspected(&compiled);
    let rendered = emission.diagnostics().render(compiled.source_map());
    assert!(
        emission
            .diagnostics()
            .kinds()
            .any(|kind| kind == DiagnosticKind::InspectionSpansDegraded),
        "{rendered}"
    );
    assert!(
        rendered.contains("inspection spans degraded: dropped pattern/reference detail"),
        "{rendered}"
    );

    let spans: Vec<_> = emission
        .artifact()
        .expect("valid query should emit")
        .spans()
        .iter()
        .collect();
    assert_eq!(spans.len(), definition_count * 2);
    assert!(
        spans
            .iter()
            .all(|span| matches!(span.kind, SpanKind::Def | SpanKind::Capture))
    );
}

#[test]
fn capture_markers_change_instruction_bytes_when_inspection_is_enabled() {
    let src = "Q = (program (expression_statement (identifier) @id))";
    let plain = QueryBuilder::from_inline(src)
        .compile(synthetic_grammar())
        .expect("query parsing should not exhaust fuel");
    let inspected = inspected(&plain)
        .into_artifact()
        .expect("inspected module emits");
    let plain = plain
        .emit(BytecodeConfig::new())
        .expect("plain bytecode emission answers")
        .into_artifact()
        .expect("plain module emits");

    assert_ne!(
        instruction_bytes(plain.bytes()),
        instruction_bytes(inspected.bytes()),
    );
    assert!(plain.spans().is_empty());
    assert!(!inspected.spans().is_empty());
}

fn inspected_spans(src: &str) -> Vec<SpanEntry> {
    let compiled = compiled(src);
    inspected(&compiled)
        .into_artifact()
        .expect("valid query should emit")
        .spans()
        .iter()
        .collect()
}

fn exact_range_query() -> &'static str {
    "Q = (program (expression_statement)? @_ (lexical_declaration (variable_declarator name: (identifier) @id :: Ident)))"
}

fn compiled(src: &str) -> CompiledQuery {
    let compiled = QueryBuilder::from_inline(src)
        .compile(synthetic_grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled
}

fn inspected(compiled: &CompiledQuery) -> Emission<crate::bytecode::Module> {
    compiled
        .emit(BytecodeConfig::new().inspection(BytecodeInspection::Spans))
        .expect("inspected bytecode emission answers")
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

fn instruction_bytes(bytes: &[u8]) -> &[u8] {
    let header = Header::from_bytes(&bytes[..HEADER_SIZE]);
    let offsets = header.compute_offsets();
    let start = offsets.instructions as usize;
    let len = header.instruction_word_count as usize * BYTECODE_WORD_SIZE;
    &bytes[start..start + len]
}
