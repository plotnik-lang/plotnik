use crate::query::QueryAnalyzed;

#[test]
fn backreference_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /(.)\1/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: backreferences are not supported in regex
      |
    1 | Q = (identifier =~ /(.)\1/)
      |                        ^^
    ");
}

#[test]
fn lookahead_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /foo(?=bar)/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: lookahead/lookbehind is not supported in regex
      |
    1 | Q = (identifier =~ /foo(?=bar)/)
      |                         ^^
    ");
}

#[test]
fn lookbehind_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /(?<=foo)bar/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: lookahead/lookbehind is not supported in regex
      |
    1 | Q = (identifier =~ /(?<=foo)bar/)
      |                      ^^^
    ");
}

#[test]
fn named_capture_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /(?P<name>foo)/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: named captures are not supported in regex
      |
    1 | Q = (identifier =~ /(?P<name>foo)/)
      |                      ^^^^^^^^
    ");
}

#[test]
fn syntax_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /[/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: invalid regex syntax: unclosed character class
      |
    1 | Q = (identifier =~ /[/)
      |                     ^
    ");
}

#[test]
fn empty_regex_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ //)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @r"
    error: empty regex pattern
      |
    1 | Q = (identifier =~ //)
      |                    ^^
    ");
}

#[test]
fn valid_regex() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /^test_/)");
    assert!(q.is_valid());
}

#[test]
fn valid_string_predicate() {
    let q = QueryAnalyzed::expect(r#"Q = (identifier == "foo")"#);
    assert!(q.is_valid());
}
