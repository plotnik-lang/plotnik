use super::*;

#[test]
fn span_kind_names_are_stable() {
    assert_eq!(SpanKind::Def.name(), "def");
    assert_eq!(SpanKind::GrammarField.name(), "grammar_field");
    assert_eq!(
        SpanKind::NegatedGrammarField.name(),
        "negated_grammar_field"
    );
    assert_eq!(
        SpanKind::Alternation(Labeling::Unlabeled).name(),
        "unlabeled_alternation"
    );
    assert_eq!(
        SpanKind::Alternation(Labeling::Labeled).name(),
        "labeled_alternation"
    );
    assert_eq!(SpanKind::CaptureType.name(), "capture_type");
    assert!(SpanKind::try_from_u8(13).is_none());
}

#[test]
fn alternation_labeling_roundtrips_through_wire_kinds() {
    for labeling in [Labeling::Unlabeled, Labeling::Labeled] {
        let entry = SpanEntry {
            source_id: 0,
            kind: SpanKind::Alternation(labeling),
            start: 0,
            end: 2,
            type_id: SPAN_NO_BINDING,
            member: SPAN_NO_BINDING,
        };

        assert_eq!(SpanEntry::from_bytes(&entry.to_bytes()), entry);
    }
}

#[test]
fn span_entry_roundtrips() {
    let entry = SpanEntry {
        source_id: 2,
        kind: SpanKind::Capture,
        start: 11,
        end: 17,
        type_id: 3,
        member: 5,
    };

    let decoded = SpanEntry::from_bytes(&entry.to_bytes());

    assert_eq!(decoded, entry);
}

#[test]
fn spans_view_decodes_entries_by_index() {
    let entries = [
        SpanEntry {
            source_id: 0,
            kind: SpanKind::Def,
            start: 0,
            end: 10,
            type_id: 1,
            member: SPAN_NO_BINDING,
        },
        SpanEntry {
            source_id: 0,
            kind: SpanKind::Capture,
            start: 6,
            end: 9,
            type_id: 1,
            member: 2,
        },
    ];
    let bytes: Vec<_> = entries.into_iter().flat_map(SpanEntry::to_bytes).collect();
    let spans = SpansView::new(&bytes, entries.len());

    assert_eq!(spans.len(), 2);
    assert_eq!(spans.get(1).kind, SpanKind::Capture);
    assert_eq!(spans.iter().collect::<Vec<_>>(), entries);
}
