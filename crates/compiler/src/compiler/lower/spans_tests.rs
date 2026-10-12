use std::fmt::Write as _;

use crate::bytecode::{MAX_SPANS, SpanKind};
use crate::compiler::diagnostics::DiagnosticKind;
use crate::compiler::test_utils::synthetic_grammar;
use crate::compiler::{BytecodeConfig, BytecodeInspection, CompiledQuery, Emission, QueryBuilder};

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
