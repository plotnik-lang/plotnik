use crate::ql::lexer::{lex, token_text};
use crate::ql::syntax_kind::SyntaxKind;

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

macro_rules! assert_lex {
    ($input:expr, @$snapshot:literal) => {
        insta::assert_snapshot!(snapshot($input), @$snapshot)
    };
}

macro_rules! assert_lex_raw {
    ($input:expr, @$snapshot:literal) => {
        insta::assert_snapshot!(snapshot_raw($input), @$snapshot)
    };
}

#[test]
fn punctuation() {
    assert_lex!("( ) [ ] { } : = ! ~ _ .", @r#"
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
    assert_lex!("{ (a) (b) }", @r#"
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
    assert_lex!("* + ?", @r#"
    Star "*"
    Plus "+"
    Question "?"
    "#);
}

#[test]
fn quantifiers_non_greedy() {
    assert_lex!("*? +? ??", @r#"
    StarQuestion "*?"
    PlusQuestion "+?"
    QuestionQuestion "??"
    "#);
}

#[test]
fn quantifiers_attached() {
    assert_lex!("foo* bar+ baz? qux*? lazy+? greedy??", @r#"
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
    assert_lex!("foo bar_baz test123", @r#"
    LowerIdent "foo"
    LowerIdent "bar_baz"
    LowerIdent "test123"
    "#);
}

#[test]
fn identifiers_upper() {
    assert_lex!("Foo BarBaz Test123", @r#"
    UpperIdent "Foo"
    UpperIdent "BarBaz"
    UpperIdent "Test123"
    "#);
}

#[test]
fn identifiers_mixed() {
    assert_lex!("Foo Bar baz test_case", @r#"
    UpperIdent "Foo"
    UpperIdent "Bar"
    LowerIdent "baz"
    LowerIdent "test_case"
    "#);
}

#[test]
fn strings_simple() {
    assert_lex!(r#""hello" "world""#, @r#"
    StringLit "\"hello\""
    StringLit "\"world\""
    "#);
}

#[test]
fn strings_with_escapes() {
    assert_lex!(r#""hello\nworld" "tab\there""#, @r#"
    StringLit "\"hello\\nworld\""
    StringLit "\"tab\\there\""
    "#);
}

#[test]
fn strings_empty() {
    assert_lex!(r#""""#, @r#"
    StringLit "\"\""
    "#);
}

#[test]
fn capture_simple() {
    assert_lex!("@name", @r#"
    At "@"
    LowerIdent "name"
    "#);
}

#[test]
fn capture_with_underscores() {
    assert_lex!("@my_capture_name", @r#"
    At "@"
    LowerIdent "my_capture_name"
    "#);
}

#[test]
fn capture_multiple() {
    assert_lex!("@name @value @other", @r#"
    At "@"
    LowerIdent "name"
    At "@"
    LowerIdent "value"
    At "@"
    LowerIdent "other"
    "#);
}

#[test]
fn capture_bare_at() {
    assert_lex!("@ foo", @r#"
    At "@"
    LowerIdent "foo"
    "#);
}

#[test]
fn capture_uppercase_not_valid() {
    // Uppercase after @ is not a valid capture - lexed as At + UpperIdent
    assert_lex!("@Name", @r#"
    At "@"
    UpperIdent "Name"
    "#);
}

#[test]
fn comment_line() {
    assert_lex_raw!("// line comment", @r#"
    LineComment "// line comment"
    "#);
}

#[test]
fn comment_block() {
    assert_lex_raw!("/* block comment */", @r#"
    BlockComment "/* block comment */"
    "#);
}

#[test]
fn comment_line_then_block() {
    assert_lex_raw!("// line comment\n/* block comment */", @r#"
    LineComment "// line comment"
    Newline "\n"
    BlockComment "/* block comment */"
    "#);
}

#[test]
fn comment_between_tokens() {
    assert_lex!("foo /* comment */ bar", @r#"
    LowerIdent "foo"
    LowerIdent "bar"
    "#);
}

#[test]
fn trivia_whitespace() {
    assert_lex_raw!("  \t ", @r#"
    Whitespace "  \t "
    "#);
}

#[test]
fn trivia_newlines() {
    assert_lex_raw!("\n\r\n", @r#"
    Newline "\n"
    Newline "\r\n"
    "#);
}

#[test]
fn trivia_mixed() {
    assert_lex_raw!("  \n\t ", @r#"
    Whitespace "  "
    Newline "\n"
    Whitespace "\t "
    "#);
}

#[test]
fn trivia_between_tokens() {
    assert_lex_raw!("foo  bar", @r#"
    LowerIdent "foo"
    Whitespace "  "
    LowerIdent "bar"
    "#);
}

#[test]
fn trivia_filtered_by_default() {
    assert_lex!("foo  bar", @r#"
    LowerIdent "foo"
    LowerIdent "bar"
    "#);
}

#[test]
fn error_coalescing() {
    assert_lex!("(foo) ^$%& (bar)", @r#"
    ParenOpen "("
    LowerIdent "foo"
    ParenClose ")"
    UnexpectedFragment "^$%&"
    ParenOpen "("
    LowerIdent "bar"
    ParenClose ")"
    "#);
}

#[test]
fn error_unexpected_xml_opening() {
    assert_lex!("<div>", @r#"UnexpectedXML "<div>""#);
}

#[test]
fn error_unexpected_xml_closing() {
    assert_lex!("</div>", @r#"UnexpectedXML "</div>""#);
}

#[test]
fn error_unexpected_xml_self_closing() {
    assert_lex!("<br/>", @r#"UnexpectedXML "<br/>""#);
}

#[test]
fn error_single_char() {
    assert_lex!("^", @r#"UnexpectedFragment "^""#);
}

#[test]
fn error_at_end() {
    assert_lex!("foo ^^^", @r#"
    LowerIdent "foo"
    UnexpectedFragment "^^^"
    "#);
}

#[test]
fn complex_pattern() {
    assert_lex!("(function_definition name: (identifier) @name)", @r#"
    ParenOpen "("
    LowerIdent "function_definition"
    LowerIdent "name"
    Colon ":"
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    At "@"
    LowerIdent "name"
    ParenClose ")"
    "#);
}

#[test]
fn alternation_pattern() {
    assert_lex!("[\"public\" \"private\" \"protected\"]", @r#"
    BracketOpen "["
    StringLit "\"public\""
    StringLit "\"private\""
    StringLit "\"protected\""
    BracketClose "]"
    "#);
}

#[test]
fn empty_input() {
    assert_lex!("", @"");
}

#[test]
fn double_colon() {
    assert_lex!("@name::Type", @r#"
    At "@"
    LowerIdent "name"
    DoubleColon "::"
    UpperIdent "Type"
    "#);
}

#[test]
fn double_colon_vs_single_colon() {
    // DoubleColon must take precedence over two Colons
    assert_lex!(":: : ::", @r#"
    DoubleColon "::"
    Colon ":"
    DoubleColon "::"
    "#);
}

#[test]
fn double_colon_string_type() {
    assert_lex!("@name::string", @r#"
    At "@"
    LowerIdent "name"
    DoubleColon "::"
    LowerIdent "string"
    "#);
}

#[test]
fn slash() {
    assert_lex!("expression/binary_expression", @r#"
    LowerIdent "expression"
    Slash "/"
    LowerIdent "binary_expression"
    "#);
}

#[test]
fn slash_vs_comment() {
    // Slash must not conflict with line comments
    assert_lex_raw!("a/b // comment", @r#"
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
    assert_lex_raw!("a/b /* comment */", @r#"
    LowerIdent "a"
    Slash "/"
    LowerIdent "b"
    Whitespace " "
    BlockComment "/* comment */"
    "#);
}

#[test]
fn keyword_error() {
    assert_lex!("(ERROR)", @r#"
    ParenOpen "("
    KwError "ERROR"
    ParenClose ")"
    "#);
}

#[test]
fn keyword_missing() {
    assert_lex!("(MISSING identifier)", @r#"
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
    assert_lex!("ERROR ERRORx Errors", @r#"
    KwError "ERROR"
    UpperIdent "ERRORx"
    UpperIdent "Errors"
    "#);
}

#[test]
fn keyword_missing_vs_upper_ident() {
    // MISSING keyword must take precedence over UpperIdent
    assert_lex!("MISSING MISSINGx Missing", @r#"
    KwMissing "MISSING"
    UpperIdent "MISSINGx"
    UpperIdent "Missing"
    "#);
}

#[test]
fn supertype_path_pattern() {
    assert_lex!("(expression/binary_expression)", @r#"
    ParenOpen "("
    LowerIdent "expression"
    Slash "/"
    LowerIdent "binary_expression"
    ParenClose ")"
    "#);
}

#[test]
fn type_annotation_full() {
    assert_lex!("(identifier) @name::string", @r#"
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    At "@"
    LowerIdent "name"
    DoubleColon "::"
    LowerIdent "string"
    "#);
}

#[test]
fn sequence_pattern() {
    assert_lex!("{ (a) (b) }*", @r#"
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
    assert_lex!("Expr = (identifier)", @r#"
    UpperIdent "Expr"
    Equals "="
    ParenOpen "("
    LowerIdent "identifier"
    ParenClose ")"
    "#);
}

#[test]
fn all_phase1_tokens_together() {
    // Comprehensive test with all new tokens
    let input = r#"
        Def = { (expr/lit) (ERROR) (MISSING ";") } @val::Type
    "#;
    let tokens = lex(input);
    let kinds: Vec<_> = tokens.iter()
        .filter(|t| !t.kind.is_trivia())
        .map(|t| t.kind)
        .collect();
    
    assert_eq!(kinds, vec![
        SyntaxKind::UpperIdent,   // Def
        SyntaxKind::Equals,       // =
        SyntaxKind::BraceOpen,    // {
        SyntaxKind::ParenOpen,    // (
        SyntaxKind::LowerIdent,   // expr
        SyntaxKind::Slash,        // /
        SyntaxKind::LowerIdent,   // lit
        SyntaxKind::ParenClose,   // )
        SyntaxKind::ParenOpen,    // (
        SyntaxKind::KwError,      // ERROR
        SyntaxKind::ParenClose,   // )
        SyntaxKind::ParenOpen,    // (
        SyntaxKind::KwMissing,    // MISSING
        SyntaxKind::StringLit,    // ";"
        SyntaxKind::ParenClose,   // )
        SyntaxKind::BraceClose,   // }
        SyntaxKind::At,           // @
        SyntaxKind::LowerIdent,   // val
        SyntaxKind::DoubleColon,  // ::
        SyntaxKind::UpperIdent,   // Type
    ]);
}