use crate::ql::parser::parse;
use crate::ql::syntax_kind::SyntaxNode;

/// Format tree without trivia tokens (default for most tests)
fn snapshot(input: &str) -> String {
    format_result(input, false)
}

/// Format tree with trivia tokens included
fn snapshot_raw(input: &str) -> String {
    format_result(input, true)
}

fn format_result(input: &str, include_trivia: bool) -> String {
    let result = parse(input);
    let mut out = String::new();
    format_tree_impl(&result.syntax(), 0, &mut out, include_trivia);
    if !result.errors().is_empty() {
        out.push_str("errors:\n");
        for err in result.errors() {
            out.push_str(&format!("  - {}\n", err));
        }
    }
    out
}

fn format_tree_impl(node: &SyntaxNode, indent: usize, out: &mut String, include_trivia: bool) {
    use std::fmt::Write;
    let prefix = "  ".repeat(indent);
    let _ = writeln!(out, "{}{:?}", prefix, node.kind());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => format_tree_impl(&n, indent + 1, out, include_trivia),
            rowan::NodeOrToken::Token(t) => {
                if include_trivia || !t.kind().is_trivia() {
                    let _ = writeln!(out, "{}  {:?} {:?}", prefix, t.kind(), t.text());
                }
            }
        }
    }
}

macro_rules! assert_parse {
    ($input:expr, @$snapshot:literal) => {
        insta::assert_snapshot!(snapshot($input), @$snapshot)
    };
}

macro_rules! assert_parse_raw {
    ($input:expr, @$snapshot:literal) => {
        insta::assert_snapshot!(snapshot_raw($input), @$snapshot)
    };
}

// =============================================================================
// Basic patterns
// =============================================================================

#[test]
fn empty_input() {
    assert_parse!("", @r#"
    Root
    "#);
}

#[test]
fn simple_named_node() {
    assert_parse!("(identifier)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn wildcard() {
    assert_parse!("(_)", @r#"
    Root
      NamedNode
        ParenOpen "("
        Underscore "_"
        ParenClose ")"
    "#);
}

#[test]
fn anonymous_node() {
    assert_parse!("\"if\"", @r#"
    Root
      AnonNode
        StringLit "\"if\""
    "#);
}

#[test]
fn anonymous_node_operator() {
    assert_parse!("\"+=\"", @r#"
    Root
      AnonNode
        StringLit "\"+=\""
    "#);
}

// =============================================================================
// Nested patterns
// =============================================================================

#[test]
fn nested_node() {
    assert_parse!("(function_definition name: (identifier))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn deeply_nested() {
    assert_parse!("(a (b (c (d))))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "a"
        NamedNode
          ParenOpen "("
          LowerIdent "b"
          NamedNode
            ParenOpen "("
            LowerIdent "c"
            NamedNode
              ParenOpen "("
              LowerIdent "d"
              ParenClose ")"
            ParenClose ")"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn sibling_children() {
    assert_parse!("(block (statement) (statement) (statement))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        ParenClose ")"
    "#);
}

// =============================================================================
// Captures
// =============================================================================

#[test]
fn capture() {
    assert_parse!("(identifier) @name", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "name"
    "#);
}

#[test]
fn capture_dotted() {
    // Dotted captures like @var.name are lexed as a single CaptureName token
    assert_parse!("(identifier) @var.name", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "var.name"
    "#);
}

#[test]
fn capture_nested() {
    assert_parse!("(call function: (identifier) @func)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "func"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_captures() {
    assert_parse!("(binary left: (_) @left right: (_) @right) @expr", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "binary"
        Field
          LowerIdent "left"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "left"
        Field
          LowerIdent "right"
          Colon ":"
          NamedNode
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "right"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "expr"
    "#);
}

// =============================================================================
// Quantifiers
// =============================================================================

#[test]
fn quantifier_star() {
    assert_parse!("(statement)*", @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Star "*"
    "#);
}

#[test]
fn quantifier_plus() {
    assert_parse!("(statement)+", @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Plus "+"
    "#);
}

#[test]
fn quantifier_optional() {
    assert_parse!("(statement)?", @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Question "?"
    "#);
}

#[test]
fn quantifier_with_capture() {
    assert_parse!("(statement)* @statements", @r#"
    Root
      Quantifier
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Star "*"
      Capture
        At "@"
        CaptureName "statements"
    "#);
}

#[test]
fn quantifier_inside_node() {
    assert_parse!("(block (statement)*)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        Quantifier
          NamedNode
            ParenOpen "("
            LowerIdent "statement"
            ParenClose ")"
          Star "*"
        ParenClose ")"
    "#);
}

// =============================================================================
// Alternation
// =============================================================================

#[test]
fn alternation() {
    assert_parse!("[(identifier) (string)]", @r#"
    Root
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn alternation_with_anonymous() {
    assert_parse!("[\"true\" \"false\"]", @r#"
    Root
      Alternation
        BracketOpen "["
        AnonNode
          StringLit "\"true\""
        AnonNode
          StringLit "\"false\""
        BracketClose "]"
    "#);
}

#[test]
fn alternation_with_capture() {
    assert_parse!("[(identifier) (string)] @value", @r#"
    Root
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
        BracketClose "]"
      Capture
        At "@"
        CaptureName "value"
    "#);
}

#[test]
fn alternation_nested() {
    assert_parse!("(expr [(binary) (unary)])", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "expr"
        Alternation
          BracketOpen "["
          NamedNode
            ParenOpen "("
            LowerIdent "binary"
            ParenClose ")"
          NamedNode
            ParenOpen "("
            LowerIdent "unary"
            ParenClose ")"
          BracketClose "]"
        ParenClose ")"
    "#);
}

// =============================================================================
// Fields
// =============================================================================

#[test]
fn field_pattern() {
    assert_parse!("(call function: (identifier))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_fields() {
    assert_parse!("(assignment left: (identifier) right: (expression))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "assignment"
        Field
          LowerIdent "left"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Field
          LowerIdent "right"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "expression"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn negated_field() {
    assert_parse!("(function !async)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function"
        NegatedField
          Negation "!"
          LowerIdent "async"
        ParenClose ")"
    "#);
}

#[test]
fn negated_and_regular_fields() {
    assert_parse!("(function !async name: (identifier))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function"
        NegatedField
          Negation "!"
          LowerIdent "async"
        Field
          LowerIdent "name"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

// =============================================================================
// Multiple patterns at root level
// =============================================================================

#[test]
fn multiple_patterns() {
    assert_parse!("(identifier) (string) (number)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      NamedNode
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
      NamedNode
        ParenOpen "("
        LowerIdent "number"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_patterns_with_captures() {
    assert_parse!("(function) @func (class) @cls", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "func"
      NamedNode
        ParenOpen "("
        LowerIdent "class"
        ParenClose ")"
      Capture
        At "@"
        CaptureName "cls"
    "#);
}

// =============================================================================
// Complex patterns
// =============================================================================

#[test]
fn complex_function_query() {
    assert_parse!(
        "(function_definition name: (identifier) @name parameters: (parameters)? body: (block (statement)*))",
        @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          CaptureName "name"
        Field
          LowerIdent "parameters"
          Colon ":"
          Quantifier
            NamedNode
              ParenOpen "("
              LowerIdent "parameters"
              ParenClose ")"
            Question "?"
        Field
          LowerIdent "body"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "block"
            Quantifier
              NamedNode
                ParenOpen "("
                LowerIdent "statement"
                ParenClose ")"
              Star "*"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn alternation_in_field() {
    assert_parse!("(call arguments: [(string) (number)])", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "arguments"
          Colon ":"
          Alternation
            BracketOpen "["
            NamedNode
              ParenOpen "("
              LowerIdent "string"
              ParenClose ")"
            NamedNode
              ParenOpen "("
              LowerIdent "number"
              ParenClose ")"
            BracketClose "]"
        ParenClose ")"
    "#);
}

// =============================================================================
// Error recovery tests
// =============================================================================

#[test]
fn error_missing_paren() {
    assert_parse!("(identifier", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
    errors:
      - error at 11..11: expected ParenClose
    "#);
}

#[test]
fn error_unexpected_token() {
    assert_parse!("(identifier) ^^^ (string)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        Error "^^^"
      NamedNode
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
    errors:
      - error at 13..16: expected pattern
    "#);
}

#[test]
fn error_missing_bracket() {
    assert_parse!("[(identifier) (string)", @r#"
    Root
      Alternation
        BracketOpen "["
        NamedNode
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
    errors:
      - error at 22..22: expected BracketClose
    "#);
}

#[test]
fn error_empty_parens() {
    // Empty parens emit an error but parsing continues
    assert_parse!("()", @r#"
    Root
      NamedNode
        ParenOpen "("
        ParenClose ")"
    errors:
      - error at 1..2: empty node pattern - expected node type or children
    "#);
}

#[test]
fn error_recovery_continues_parsing() {
    assert_parse!("(a (b) @@@ (c)) (d)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "a"
        NamedNode
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        Capture
          At "@"
        Capture
          At "@"
        Capture
          At "@"
        NamedNode
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
        ParenClose ")"
      NamedNode
        ParenOpen "("
        LowerIdent "d"
        ParenClose ")"
    errors:
      - error at 8..9: expected capture name
      - error at 9..10: expected capture name
      - error at 11..12: expected capture name
    "#);
}

#[test]
fn error_missing_capture_name() {
    assert_parse!("(identifier) @", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
    errors:
      - error at 14..14: expected capture name
    "#);
}

#[test]
fn error_missing_field_value() {
    assert_parse!("(call name:)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "name"
          Colon ":"
          Error
            ParenClose ")"
    errors:
      - error at 11..12: expected pattern
      - error at 12..12: expected ParenClose
    "#);
}

// =============================================================================
// Trivia tests
// =============================================================================

#[test]
fn trivia_whitespace_preserved() {
    assert_parse_raw!("(identifier)  @name", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Whitespace "  "
      Capture
        At "@"
        CaptureName "name"
    "#);
}

#[test]
fn trivia_comment_preserved() {
    assert_parse_raw!("// comment\n(identifier)", @r#"
    Root
      LineComment "// comment"
      Newline "\n"
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn trivia_multiline() {
    assert_parse_raw!("(a)\n\n(b)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
      Newline "\n"
      Newline "\n"
      NamedNode
        ParenOpen "("
        LowerIdent "b"
        ParenClose ")"
    "#);
}

#[test]
fn trivia_comment_inside_pattern() {
    assert_parse_raw!("(call // inline\n  name: (identifier))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "call"
        Whitespace " "
        LineComment "// inline"
        Newline "\n"
        Whitespace "  "
        Field
          LowerIdent "name"
          Colon ":"
          Whitespace " "
          NamedNode
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn trivia_filtered_by_default() {
    // Same input but without trivia - should be clean
    assert_parse!("// comment\n(identifier)", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn trivia_between_alternation_items() {
    assert_parse_raw!("[\n  (a)\n  (b)\n]", @r#"
    Root
      Alternation
        BracketOpen "["
        Newline "\n"
        Whitespace "  "
        NamedNode
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Newline "\n"
        Whitespace "  "
        NamedNode
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BracketClose "]"
      Newline "\n"
    "#);
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn whitespace_only() {
    assert_parse_raw!("   \n\n   ", @r#"
    Root
      Whitespace "   "
      Newline "\n"
      Newline "\n"
      Whitespace "   "
    "#);
}

#[test]
fn comment_only_raw() {
    assert_parse_raw!("// just a comment", @r#"
    Root
      LineComment "// just a comment"
    "#);
}

#[test]
fn anchor_dot() {
    assert_parse!("(block . (first_statement))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        Anchor
          Dot "."
        NamedNode
          ParenOpen "("
          LowerIdent "first_statement"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn mixed_children_and_fields() {
    assert_parse!("(if condition: (expr) (then_block) else: (else_block))", @r#"
    Root
      NamedNode
        ParenOpen "("
        LowerIdent "if"
        Field
          LowerIdent "condition"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "expr"
            ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "then_block"
          ParenClose ")"
        Field
          LowerIdent "else"
          Colon ":"
          NamedNode
            ParenOpen "("
            LowerIdent "else_block"
            ParenClose ")"
        ParenClose ")"
    "#);
}
