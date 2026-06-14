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
      |
    help: the regex engine is linear-time and cannot match backreferences; rewrite without `\1`
    ");
}

#[test]
fn lookahead_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /foo(?=bar)/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @"
    error: lookahead/lookbehind is not supported in regex
      |
    1 | Q = (identifier =~ /foo(?=bar)/)
      |                         ^^
      |
    help: the regex engine cannot match lookaround; match the surrounding context with the query pattern instead
    ");
}

#[test]
fn lookbehind_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /(?<=foo)bar/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @"
    error: lookahead/lookbehind is not supported in regex
      |
    1 | Q = (identifier =~ /(?<=foo)bar/)
      |                      ^^^
      |
    help: the regex engine cannot match lookaround; match the surrounding context with the query pattern instead
    ");
}

#[test]
fn named_capture_error() {
    let q = QueryAnalyzed::expect(r"Q = (identifier =~ /(?P<name>foo)/)");
    assert!(!q.is_valid());
    insta::assert_snapshot!(q.dump_diagnostics(), @"
    error: named captures are not supported in regex
      |
    1 | Q = (identifier =~ /(?P<name>foo)/)
      |                      ^^^^^^^^
      |
    help: remove the named-capture marker
      |
    1 - Q = (identifier =~ /(?P<name>foo)/)
    1 + Q = (identifier =~ /(foo)/)
      |
    help: regex captures are inert in plotnik; capture nodes with `@name` outside the regex
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
    insta::assert_snapshot!(q.dump_diagnostics(), @r#"
    error: empty regex pattern
      |
    1 | Q = (identifier =~ //)
      |                    ^^
      |
    help: put a pattern between the slashes, e.g. `=~ /^foo/`, or use a string predicate like `== "foo"`
    "#);
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
