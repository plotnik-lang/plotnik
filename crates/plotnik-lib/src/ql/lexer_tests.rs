use crate::ql::lexer::{lex, token_text};

/// Format tokens without trivia (default for most tests)
fn snapshot(input: &str) -> String {
    format_tokens(input, false)
}

/// Format tokens with trivia included
fn snapshot_raw(input: &str) -> String {
    format_tokens(input, true)
}

fn format_tokens(input: &str, include_trivia: bool) -> String {
    let tokens = lex(input);
    let mut out = String::new();
    for token in tokens {
        if include_trivia || !token.kind.is_trivia() {
            out.push_str(&format!(
                "{:?} {:?}\n",
                token.kind,
                token_text(input, &token)
            ));
        }
    }
    out
}

#[test]
fn punctuation() {
    insta::assert_snapshot!(snapshot("( ) [ ] { } : = ! ~ _ ."), @r#"
    ParenOpen "("
    ParenClose ")"
    BracketOpen "["
    BracketClose "]"
    BraceOpen "{"
    BraceClose "}"
    Colon ":"
    Equals "="
    Negation "!"
    Tilde "~"
    Underscore "_"
    Dot "."
    "#);
}

#[test]
fn braces() {
    insta::assert_snapshot!(snapshot("{ (a) (b) }"), @r#"
    BraceOpen "{"
    ParenOpen "("
    LowerIdent "a"
    ParenClose ")"
    ParenOpen "("
    LowerIdent "b"
    ParenClose ")"
    BraceClose "}"
    "#);
}

#[test]
fn quantifiers_greedy() {
    insta::assert_snapshot!(snapshot("* + ?"), @r#"
    Star "*"
    Plus "+"
    Question "?"
    "#);
}

#[test]
fn quantifiers_non_greedy() {
    insta::assert_snapshot!(snapshot("*? +? ??"), @r#"
    StarQuestion "*?"
    PlusQuestion "+?"
    QuestionQuestion "??"
    "#);
}

#[test]
fn quantifiers_attached() {
    insta::assert_snapshot!(snapshot("foo* bar+ baz? qux*? lazy+? greedy??"), @r#"
    LowerIdent "foo"
    Star "*"
    LowerIdent "bar"
    Plus "+"
    LowerIdent "baz"
    Question "?"
    LowerIdent "qux"
    StarQuestion "*?"
    LowerIdent "lazy"
    PlusQuestion "+?"
    LowerIdent "greedy"
    QuestionQuestion "??"
    "#);
}

#[test]
fn identifiers_lower() {
    insta::assert_snapshot!(snapshot("foo bar_baz test123"), @r#"
    LowerIdent "foo"
    LowerIdent "bar_baz"
    LowerIdent "test123"
    "#);
}

#[test]
fn identifiers_upper() {
    insta::assert_snapshot!(snapshot("Foo BarBaz Test123"), @r#"
    UpperIdent "Foo"
    UpperIdent "BarBaz"
    UpperIdent "Test123"
    "#);
}

#[test]
fn identifiers_mixed() {
    insta::assert_snapshot!(snapshot("Foo Bar baz test_case"), @r#"
    UpperIdent "Foo"
    UpperIdent "Bar"
    LowerIdent "baz"
    LowerIdent "test_case"
    "#);
}

#[test]
fn strings_simple() {
    insta::assert_snapshot!(snapshot(r#""hello" "world""#), @r#"
    StringLit "\"hello\""
    StringLit "\"world\""
    "#);
}

#[test]
fn strings_with_escapes() {
    insta::assert_snapshot!(snapshot(r#""hello\nworld" "tab\there""#), @r#"
    StringLit "\"hello\\nworld\""
    StringLit "\"tab\\there\""
    "#);
}

#[test]
fn strings_empty() {
    insta::assert_snapshot!(snapshot(r#""""#), @r#"StringLit "\"\"""#);
}

#[test]
fn capture_simple() {
    insta::assert_snapshot!(snapshot("@name"), @r#"CaptureName "@name""#);
}

#[test]
fn capture_with_underscores() {
    insta::assert_snapshot!(snapshot("@my_capture_name"), @r#"CaptureName "@my_capture_name""#);
}

#[test]
fn capture_multiple() {
    insta::assert_snapshot!(snapshot("@name @value @other"), @r#"
    CaptureName "@name"
    CaptureName "@value"
    CaptureName "@other"
    "#);
}

#[test]
fn capture_bare_at() {
    insta::assert_snapshot!(snapshot("@ foo"), @r#"
    Garbage "@"
    LowerIdent "foo"
    "#);
}

#[test]
fn capture_uppercase_not_valid() {
    // Uppercase after @ is not a valid capture - lexed as At + UpperIdent
    insta::assert_snapshot!(snapshot("@Name"), @r#"CaptureName "@Name""#);
}

#[test]
fn comment_line() {
    insta::assert_snapshot!(snapshot_raw("// line comment"), @r#"LineComment "// line comment""#);
}

#[test]
fn comment_block() {
    insta::assert_snapshot!(snapshot_raw("/* block comment */"), @r#"BlockComment "/* block comment */""#);
}

#[test]
fn comment_line_then_block() {
    insta::assert_snapshot!(snapshot_raw("// line comment\n/* block comment */"), @r#"
    LineComment "// line comment"
    Newline "\n"
    BlockComment "/* block comment */"
    "#);
}

#[test]
fn comment_between_tokens() {
    insta::assert_snapshot!(snapshot("foo /* comment */ bar"), @r#"
    LowerIdent "foo"
    LowerIdent "bar"
    "#);
}

#[test]
fn trivia_whitespace() {
    insta::assert_snapshot!(snapshot_raw("  \t "), @r#"Whitespace "  \t ""#);
}

#[test]
fn trivia_newlines() {
    insta::assert_snapshot!(snapshot_raw("\n\r\n"), @r#"
    Newline "\n"
    Newline "\r\n"
    "#);
}

#[test]
fn trivia_mixed() {
    insta::assert_snapshot!(snapshot_raw("  \n\t "), @r#"
    Whitespace "  "
    Newline "\n"
    Whitespace "\t "
    "#);
}

#[test]
fn trivia_between_tokens() {
    insta::assert_snapshot!(snapshot_raw("foo  bar"), @r#"
    LowerIdent "foo"
    Whitespace "  "
    LowerIdent "bar"
    "#);
}

#[test]
fn trivia_filtered_by_default() {
    insta::assert_snapshot!(snapshot("foo  bar"), @r#"
    LowerIdent "foo"
    LowerIdent "bar"
    "#);
}

#[test]
fn error_coalescing() {
    insta::assert_snapshot!(snapshot("(foo) ^$%& (bar)"), @r#"
    ParenOpen "("
    LowerIdent "foo"
    ParenClose ")"
    Garbage "^$%&"
    ParenOpen "("
    LowerIdent "bar"
    ParenClose ")"
    "#);
}

#[test]
fn error_unexpected_xml_opening() {
    insta::assert_snapshot!(snapshot("<div>"), @r#"XMLGarbage "<div>""#);
}

#[test]
fn error_unexpected_xml_closing() {
    insta::assert_snapshot!(snapshot("</div>"), @r#"XMLGarbage "</div>""#);
}

#[test]
fn error_unexpected_xml_self_closing() {
    insta::assert_snapshot!(snapshot("<br/>"), @r#"XMLGarbage "<br/>""#);
}

#[test]
fn error_predicate_eq() {
    insta::assert_snapshot!(snapshot("#eq?"), @r##"Predicate "#eq?""##);
}

#[test]
fn error_predicate_match() {
    insta::assert_snapshot!(snapshot("#match?"), @r##"Predicate "#match?""##);
}

#[test]
fn error_predicate_set() {
    insta::assert_snapshot!(snapshot("#set!"), @r##"Predicate "#set!""##);
}

#[test]
fn error_predicate_no_suffix() {
    insta::assert_snapshot!(snapshot("#is_not"), @r##"Predicate "#is_not""##);
}

#[test]
fn error_single_char() {
    insta::assert_snapshot!(snapshot("^"), @r#"Garbage "^""#);
}

#[test]
fn error_at_end() {
    insta::assert_snapshot!(snapshot("foo ^^^"), @r#"
    LowerIdent "foo"
    Garbage "^^^"
    "#);
}

#[test]
fn complex_pattern() {
    insta::assert_snapshot!(snapshot("(function_definition name: (identifier) @name)"), @r#"
    ParenOpen "("
    LowerIdent "function_definition"
    LowerIdent "name"
    Colon ":"
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    CaptureName "@name"
    ParenClose ")"
    "#);
}

#[test]
fn alternation_pattern() {
    insta::assert_snapshot!(snapshot("[\"public\" \"private\" \"protected\"]"), @r#"
    BracketOpen "["
    StringLit "\"public\""
    StringLit "\"private\""
    StringLit "\"protected\""
    BracketClose "]"
    "#);
}

#[test]
fn empty_input() {
    insta::assert_snapshot!(snapshot(""), @"");
}

#[test]
fn double_colon() {
    insta::assert_snapshot!(snapshot("@name :: Type"), @r#"
    CaptureName "@name"
    DoubleColon "::"
    UpperIdent "Type"
    "#);
}

#[test]
fn double_colon_no_spaces() {
    insta::assert_snapshot!(snapshot("@name::Type"), @r#"
    CaptureName "@name"
    DoubleColon "::"
    UpperIdent "Type"
    "#);
}

#[test]
fn double_colon_vs_single_colon() {
    // DoubleColon must take precedence over two Colons
    insta::assert_snapshot!(snapshot(":: : ::"), @r#"
    DoubleColon "::"
    Colon ":"
    DoubleColon "::"
    "#);
}

#[test]
fn double_colon_string_type() {
    insta::assert_snapshot!(snapshot("@name :: string"), @r#"
    CaptureName "@name"
    DoubleColon "::"
    LowerIdent "string"
    "#);
}

#[test]
fn slash() {
    insta::assert_snapshot!(snapshot("expression/binary_expression"), @r#"
    LowerIdent "expression"
    Slash "/"
    LowerIdent "binary_expression"
    "#);
}

#[test]
fn slash_vs_comment() {
    // Slash must not conflict with line comments
    insta::assert_snapshot!(snapshot_raw("a/b // comment"), @r#"
    LowerIdent "a"
    Slash "/"
    LowerIdent "b"
    Whitespace " "
    LineComment "// comment"
    "#);
}

#[test]
fn slash_vs_block_comment() {
    // Slash must not conflict with block comments
    insta::assert_snapshot!(snapshot_raw("a/b /* comment */"), @r#"
    LowerIdent "a"
    Slash "/"
    LowerIdent "b"
    Whitespace " "
    BlockComment "/* comment */"
    "#);
}

#[test]
fn keyword_error() {
    insta::assert_snapshot!(snapshot("(ERROR)"), @r#"
    ParenOpen "("
    KwError "ERROR"
    ParenClose ")"
    "#);
}

#[test]
fn keyword_missing() {
    insta::assert_snapshot!(snapshot("(MISSING identifier)"), @r#"
    ParenOpen "("
    KwMissing "MISSING"
    LowerIdent "identifier"
    ParenClose ")"
    "#);
}

#[test]
fn keyword_error_vs_upper_ident() {
    // ERROR keyword must take precedence over UpperIdent
    // But ERRORx should be UpperIdent
    insta::assert_snapshot!(snapshot("ERROR ERRORx Errors"), @r#"
    KwError "ERROR"
    UpperIdent "ERRORx"
    UpperIdent "Errors"
    "#);
}

#[test]
fn keyword_missing_vs_upper_ident() {
    // MISSING keyword must take precedence over UpperIdent
    insta::assert_snapshot!(snapshot("MISSING MISSINGx Missing"), @r#"
    KwMissing "MISSING"
    UpperIdent "MISSINGx"
    UpperIdent "Missing"
    "#);
}

#[test]
fn supertype_path_pattern() {
    insta::assert_snapshot!(snapshot("(expression/binary_expression)"), @r#"
    ParenOpen "("
    LowerIdent "expression"
    Slash "/"
    LowerIdent "binary_expression"
    ParenClose ")"
    "#);
}

#[test]
fn type_annotation_full() {
    insta::assert_snapshot!(snapshot("(identifier) @name :: string"), @r#"
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    CaptureName "@name"
    DoubleColon "::"
    LowerIdent "string"
    "#);
}

#[test]
fn sequence_pattern() {
    insta::assert_snapshot!(snapshot("{ (a) (b) }*"), @r#"
    BraceOpen "{"
    ParenOpen "("
    LowerIdent "a"
    ParenClose ")"
    ParenOpen "("
    LowerIdent "b"
    ParenClose ")"
    BraceClose "}"
    Star "*"
    "#);
}

#[test]
fn named_def_tokens() {
    insta::assert_snapshot!(snapshot("Expr = (identifier)"), @r#"
    UpperIdent "Expr"
    Equals "="
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    "#);
}

#[test]
fn special_node_error() {
    insta::assert_snapshot!(snapshot("(ERROR)"), @r#"
    ParenOpen "("
    KwError "ERROR"
    ParenClose ")"
    "#);
}

#[test]
fn special_node_missing() {
    insta::assert_snapshot!(snapshot("(MISSING)"), @r#"
    ParenOpen "("
    KwMissing "MISSING"
    ParenClose ")"
    "#);
}

#[test]
fn special_node_missing_with_arg() {
    insta::assert_snapshot!(snapshot(r#"(MISSING ";")"#), @r#"
    ParenOpen "("
    KwMissing "MISSING"
    StringLit "\";\""
    ParenClose ")"
    "#);
}

#[test]
fn type_annotation_upper() {
    insta::assert_snapshot!(snapshot("@val :: Type"), @r#"
    CaptureName "@val"
    DoubleColon "::"
    UpperIdent "Type"
    "#);
}

#[test]
fn named_def_with_sequence() {
    insta::assert_snapshot!(snapshot("Def = { (a) (b) }"), @r#"
    UpperIdent "Def"
    Equals "="
    BraceOpen "{"
    ParenOpen "("
    LowerIdent "a"
    ParenClose ")"
    ParenOpen "("
    LowerIdent "b"
    ParenClose ")"
    BraceClose "}"
    "#);
}

#[test]
fn comma_token() {
    insta::assert_snapshot!(snapshot("a, b, c"), @r#"
    LowerIdent "a"
    Comma ","
    LowerIdent "b"
    Comma ","
    LowerIdent "c"
    "#);
}

#[test]
fn pipe_token() {
    insta::assert_snapshot!(snapshot("a | b | c"), @r#"
    LowerIdent "a"
    Pipe "|"
    LowerIdent "b"
    Pipe "|"
    LowerIdent "c"
    "#);
}

#[test]
fn single_quote_string() {
    insta::assert_snapshot!(snapshot("'hello'"), @r#"SingleQuoteLit "'hello'""#);
}

#[test]
fn single_quote_string_with_escape() {
    insta::assert_snapshot!(snapshot(r"'it\'s'"), @r#"SingleQuoteLit "'it\\'s'""#);
}

#[test]
fn single_vs_double_quote_strings() {
    insta::assert_snapshot!(snapshot(r#"'single' "double""#), @r#"
    SingleQuoteLit "'single'"
    StringLit "\"double\""
    "#);
}

#[test]
fn comma_in_pattern_context() {
    insta::assert_snapshot!(snapshot("[(a), (b)]"), @r#"
    BracketOpen "["
    ParenOpen "("
    LowerIdent "a"
    ParenClose ")"
    Comma ","
    ParenOpen "("
    LowerIdent "b"
    ParenClose ")"
    BracketClose "]"
    "#);
}

#[test]
fn pipe_in_pattern_context() {
    insta::assert_snapshot!(snapshot("[a | b]"), @r#"
    BracketOpen "["
    LowerIdent "a"
    Pipe "|"
    LowerIdent "b"
    BracketClose "]"
    "#);
}
