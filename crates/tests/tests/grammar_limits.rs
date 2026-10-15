use indoc::indoc;
use plotnik_compiler::diagnostics::JsonDiagnostic;
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{CompiledQuery, DiagnosticKind, QueryBuilder, SourceMap, SourcePath};

const REPEATED_IDS: &str = indoc! {"
    rules:
      args:
        type: REPEAT
        content: {type: SYMBOL, name: id}
      id: {type: PATTERN, value: '[a-z]+'}
"};

const BRACKETED_IDS: &str = indoc! {"
    rules:
      list:
        type: SEQ
        members:
          - {type: STRING, value: '['}
          - {type: SYMBOL, name: id}
          - {type: STRING, value: ']'}
      id: {type: PATTERN, value: '[a-z]+'}
"};

fn grammar(yaml: &str) -> Grammar {
    let raw: RawGrammar = serde_saphyr::from_str(&format!("name: limits\n{yaml}"))
        .expect("test grammar is valid YAML");
    Grammar::from_raw(&raw).expect("test grammar is valid")
}

fn query(text: &str) -> QueryBuilder {
    let mut sources = SourceMap::new();
    sources.add_file(SourcePath::new("query.ptk"), text);
    QueryBuilder::new(sources)
}

fn diagnostic(compiled: &CompiledQuery) -> JsonDiagnostic {
    assert!(!compiled.is_valid());
    let mut diagnostics = compiled.diagnostics().to_wire(compiled.source_map());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    diagnostics.pop().expect("one diagnostic")
}

fn assert_span(diagnostic: &JsonDiagnostic, start: (u32, u32, u32), end: (u32, u32, u32)) {
    assert_eq!(diagnostic.span.file, "query.ptk");
    let span = &diagnostic.span;
    assert_eq!(
        (span.start.line, span.start.column, span.start.offset),
        start
    );
    assert_eq!((span.end.line, span.end.column, span.end.offset), end);
}

#[test]
fn solve_limit() {
    let grammar = grammar(REPEATED_IDS);

    let compiled = query("Q = (args (id))")
        .with_satisfiability_work_budget(64)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::QueryTooComplex);
    assert_eq!(
        diagnostic.message,
        "grammar analysis exhausted its work budget"
    );
    assert_eq!(
        diagnostic.hints,
        ["reduce the child constraints or increase the grammar-analysis work budget"]
    );
    assert_span(&diagnostic, (1, 5, 4), (1, 16, 15));
}

#[test]
fn diagnostic_construction_depth() {
    let grammar = grammar(REPEATED_IDS);
    let text = "Name = (id)\nGroup = {(Name)}\nQ = (args (Group))";

    let compiled = query(text)
        .with_satisfiability_work_budget(2_000_000)
        .with_satisfiability_automaton_max_depth(2)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::QueryTooComplex);
    assert_eq!(
        diagnostic.message,
        "grammar analysis exceeded its pattern construction depth limit"
    );
    assert_eq!(
        diagnostic.hints,
        ["reduce nesting in child patterns and definition references"]
    );
    assert_span(&diagnostic, (3, 5, 33), (3, 19, 47));
}

#[test]
fn unknown_field_before_limit() {
    let grammar = grammar(indoc! {"
        rules:
          param:
            type: FIELD
            name: name
            content: {type: SYMBOL, name: id}
          id: {type: PATTERN, value: '[a-z]+'}
    "});

    let compiled = query("Q = (param nme: (id))")
        .with_satisfiability_work_budget(0)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::UnknownGrammarField);
    assert_span(&diagnostic, (1, 12, 11), (1, 15, 14));
    assert_eq!(diagnostic.hints, ["did you mean the field `name`?"]);
}

#[test]
fn known_failure_before_limit() {
    let grammar = grammar(BRACKETED_IDS);
    let text = "Q = (list .! (id))\nOther = (list (id))";

    let compiled = query(text)
        .with_satisfiability_work_budget(100)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::UnsatisfiablePattern);
    assert_eq!(
        diagnostic.message,
        "the grammar builds no `list` matching this child structure"
    );
    assert_span(&diagnostic, (1, 6, 5), (1, 10, 9));
}

#[test]
fn explanation_limit() {
    let grammar = grammar(BRACKETED_IDS);

    let compiled = query("Q = (list .! (id))")
        .with_satisfiability_work_budget(100)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::UnsatisfiablePattern);
    assert_eq!(
        diagnostic.message,
        "the grammar builds no `list` matching this child structure"
    );
    assert_span(&diagnostic, (1, 6, 5), (1, 10, 9));
}

#[test]
fn explanation_nested_limit() {
    let grammar = grammar(indoc! {"
        rules:
          list:
            type: SEQ
            members:
              - {type: STRING, value: '['}
              - {type: SYMBOL, name: pair}
              - {type: STRING, value: ']'}
          pair:
            type: SEQ
            members:
              - {type: SYMBOL, name: id}
              - {type: SYMBOL, name: num}
          id: {type: PATTERN, value: '[a-z]+'}
          num: {type: PATTERN, value: '[0-9]+'}
    "});
    let text = "Q = (list\n  .!\n  (pair\n    (num)\n    (id)\n  )\n)";

    let compiled = query(text)
        .with_satisfiability_work_budget(200)
        .compile(&grammar)
        .unwrap();

    let diagnostic = diagnostic(&compiled);
    assert_eq!(diagnostic.code, DiagnosticKind::UnsatisfiablePattern);
    assert_eq!(
        diagnostic.message,
        "the grammar builds no `list` matching this child structure"
    );
    assert_span(&diagnostic, (1, 6, 5), (1, 10, 9));
}
