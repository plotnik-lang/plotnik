#![allow(dead_code)] // TODO: remove later

use logos::Logos;
use std::ops::Range;

#[derive(Logos)]
#[cfg_attr(test, derive(serde::Serialize))]
#[derive(Debug, PartialEq, Clone)]
pub enum Token<'src> {
    #[token("(")]
    ParenOpen,

    #[token(")")]
    ParenClose,

    #[token("[")]
    BracketOpen,

    #[token("]")]
    BracketClose,

    #[token(":")]
    Colon,

    #[token("=")]
    Equals,

    #[token("!")]
    Negation,

    #[token("~")]
    Tilde,

    #[token("_")]
    Underscore,

    #[token("*?")]
    StarQuestion,

    #[token("+?")]
    PlusQuestion,

    #[token("??")]
    QuestionQuestion,

    #[token("*")]
    Star,

    #[token("+")]
    Plus,

    #[token("?")]
    Question,

    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    String(&'src str),

    #[regex(r"[A-Z][A-Za-z0-9]*")]
    UpperIdentifier(&'src str),

    #[regex(r"[a-z][a-z0-9_]*")]
    LowerIdentifier(&'src str),

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment(&'src str),

    #[regex(r"//[^\n]*")]
    LineComment(&'src str),

    #[regex(r"[ \t]+")]
    Whitespace(&'src str),

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*(?:\s+[^>]*)?>")]
    #[regex(r"</[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*>")]
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*/\s*>")]
    UnexpectedXML(&'src str),

    UnexpectedFragment(&'src str),
}

pub struct TokenStream<'src> {
    lexer: logos::Lexer<'src, Token<'src>>,
    src: &'src str,
    error_span: Option<Range<usize>>,
    pending_token: Option<Token<'src>>,
}

impl<'src> TokenStream<'src> {
    pub fn new(src: &'src str) -> Self {
        Self {
            lexer: Token::lexer(src),
            src,
            error_span: None,
            pending_token: None,
        }
    }
}

impl<'src> Iterator for TokenStream<'src> {
    type Item = Token<'src>;

    fn next(&mut self) -> Option<Token<'src>> {
        if let Some(token) = self.pending_token.take() {
            return Some(token);
        }

        loop {
            match self.lexer.next() {
                Some(Ok(token)) => {
                    if let Some(span) = self.error_span.take() {
                        let fragment = &self.src[span];
                        self.pending_token = Some(token);
                        return Some(Token::UnexpectedFragment(fragment));
                    }
                    return Some(token);
                }
                Some(Err(())) => {
                    let span = self.lexer.span();
                    match &mut self.error_span {
                        None => {
                            self.error_span = Some(span);
                        }
                        Some(existing) => {
                            existing.end = span.end;
                        }
                    }
                }
                None => {
                    if let Some(span) = self.error_span.take() {
                        let fragment = &self.src[span];
                        return Some(Token::UnexpectedFragment(fragment));
                    }
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter_whitespace(tokens: Vec<Token>) -> Vec<Token> {
        tokens
            .into_iter()
            .filter(|t| !matches!(t, Token::Whitespace(_)))
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        let stream = TokenStream::new("( ) [ ] : = ! ~ _ *? +? ?? * + ?");
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - ParenOpen
        - ParenClose
        - BracketOpen
        - BracketClose
        - Colon
        - Equals
        - Negation
        - Tilde
        - Underscore
        - StarQuestion
        - PlusQuestion
        - QuestionQuestion
        - Star
        - Plus
        - Question
        "#);
    }

    #[test]
    fn test_identifiers() {
        let stream = TokenStream::new("Foo Bar baz test_case snake_case CamelCase");
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - UpperIdentifier: Foo
        - UpperIdentifier: Bar
        - LowerIdentifier: baz
        - LowerIdentifier: test_case
        - LowerIdentifier: snake_case
        - UpperIdentifier: CamelCase
        "#);
    }

    #[test]
    fn test_strings() {
        let stream = TokenStream::new(r#""hello" "world" "escaped \"quote\"" "with\\backslash""#);
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - String: "\"hello\""
        - String: "\"world\""
        - String: "\"escaped \\\"quote\\\"\""
        - String: "\"with\\\\backslash\""
        "#);
    }

    #[test]
    fn test_comments() {
        let stream = TokenStream::new("// line comment\n/* block comment */");
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r"
        - LineComment: // line comment
        - Newline
        - BlockComment: /* block comment */
        ");
    }

    #[test]
    fn test_query_example() {
        let input = r#"Foo(bar: "baz", test*)"#;
        let stream = TokenStream::new(input);
        let tokens: Vec<_> = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - UpperIdentifier: Foo
        - ParenOpen
        - LowerIdentifier: bar
        - Colon
        - String: "\"baz\""
        - UnexpectedFragment: ","
        - LowerIdentifier: test
        - Star
        - ParenClose
        "#);
    }

    #[test]
    fn test_whitespace_and_newlines() {
        let input = "foo  \n  bar\r\n\tbaz";
        let stream = TokenStream::new(input);
        let tokens: Vec<_> = stream.collect();
        insta::assert_yaml_snapshot!(tokens, @r#"
        - LowerIdentifier: foo
        - Whitespace: "  "
        - Newline
        - Whitespace: "  "
        - LowerIdentifier: bar
        - Newline
        - Whitespace: "\t"
        - LowerIdentifier: baz
        "#);
    }

    #[test]
    fn test_quantifiers() {
        let stream = TokenStream::new("foo* bar+ baz? qux*? lazy+? greedy??");
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - LowerIdentifier: foo
        - Star
        - LowerIdentifier: bar
        - Plus
        - LowerIdentifier: baz
        - Question
        - LowerIdentifier: qux
        - StarQuestion
        - LowerIdentifier: lazy
        - PlusQuestion
        - LowerIdentifier: greedy
        - QuestionQuestion
        "#);
    }

    #[test]
    fn test_unexpected_xml() {
        let input = r#"<div> </div> <MyTag attr="value"> <self-closing/> <tag />"#;
        let stream = TokenStream::new(input);
        let tokens = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r#"
        - UnexpectedXML: "<div>"
        - UnexpectedXML: "</div>"
        - UnexpectedXML: "<MyTag attr=\"value\">"
        - UnexpectedXML: "<self-closing/>"
        - UnexpectedXML: "<tag />"
        "#);
    }

    #[test]
    fn test_error() {
        let input = r#"(foo) ^$%& (bar)"#;
        let stream = TokenStream::new(input);
        let tokens: Vec<_> = filter_whitespace(stream.collect());
        insta::assert_yaml_snapshot!(tokens, @r"
        - ParenOpen
        - LowerIdentifier: foo
        - ParenClose
        - UnexpectedFragment: ^$%&
        - ParenOpen
        - LowerIdentifier: bar
        - ParenClose
        ");
    }
}
