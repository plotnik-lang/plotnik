//! Tyton: Types Testing Object Notation
//!
//! A compact DSL for constructing `TypeTable` test fixtures.
//! Supports both parsing (text → TypeTable) and emitting (TypeTable → text).
//!
//! # Design
//!
//! Tyton uses a **flattened structure** mirroring `TypeTable`: all types are
//! top-level definitions referenced by name. No inline nesting is supported.
//!
//! ```text
//! // ✗ Invalid: inline optional
//! Foo = { #Node? @maybe }
//!
//! // ✓ Valid: separate definition + reference
//! MaybeNode = #Node?
//! Foo = { MaybeNode @maybe }
//! ```
//!
//! # Syntax
//!
//! Keys:
//! - `#Node` — built-in node type
//! - `#string` — built-in string type
//! - `#Invalid` — built-in invalid type
//! - `()` — built-in unit type
//! - `PascalName` — named type
//! - `<Foo bar baz>` — synthetic key from path segments
//!
//! Values:
//! - `{ Type @field ... }` — struct with fields
//! - `[ Tag: Type ... ]` — tagged union
//! - `Key?` — optional wrapper
//! - `Key*` — list wrapper
//! - `Key+` — non-empty list wrapper
//! - `#Node` / `#string` / `()` — bare builtin alias
//!
//! Definitions:
//! - `Name = { ... }` — define a struct
//! - `Name = [ ... ]` — define a tagged union
//! - `Name = Other?` — define an optional
//! - `<Foo bar> = { ... }` — define with synthetic key
//! - `AliasNode = #Node` — alias to builtin
//!
//! # Example
//!
//! ```text
//! FuncInfo = { #string @name #Node @body }
//! Stmt = [ Assign: AssignStmt Call: CallStmt ]
//! Stmts = Stmt*
//! ```

use std::fmt::Write;

use indexmap::IndexMap;
use logos::Logos;

use super::{TypeKey, TypeTable, TypeValue};

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\n\r]+")]
enum Token<'src> {
    // Built-in type keywords (prefixed with #)
    #[token("#Node")]
    Node,

    #[token("#string")]
    String,

    #[token("#Invalid")]
    Invalid,

    #[token("()")]
    Unit,

    // Symbols
    #[token("=")]
    Eq,

    #[token("{")]
    LBrace,

    #[token("}")]
    RBrace,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token("<")]
    LAngle,

    #[token(">")]
    RAngle,

    #[token(":")]
    Colon,

    #[token("@")]
    At,

    #[token("?")]
    Question,

    #[token("*")]
    Star,

    #[token("+")]
    Plus,

    // Identifiers: PascalCase for type names, snake_case for fields/segments
    #[regex(r"[A-Z][a-zA-Z0-9]*", |lex| lex.slice())]
    UpperIdent(&'src str),

    #[regex(r"[a-z][a-z0-9_]*", |lex| lex.slice())]
    LowerIdent(&'src str),
}

struct Parser<'src> {
    tokens: Vec<(Token<'src>, std::ops::Range<usize>)>,
    pos: usize,
    input: &'src str,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: std::ops::Range<usize>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {:?}", self.message, self.span)
    }
}

impl std::error::Error for ParseError {}

impl<'src> Parser<'src> {
    fn new(input: &'src str) -> Result<Self, ParseError> {
        let lexer = Token::lexer(input);
        let mut tokens = Vec::new();

        for (result, span) in lexer.spanned() {
            match result {
                Ok(token) => tokens.push((token, span)),
                Err(_) => {
                    return Err(ParseError {
                        message: format!("unexpected character: {:?}", &input[span.clone()]),
                        span,
                    });
                }
            }
        }

        Ok(Self {
            tokens,
            pos: 0,
            input,
        })
    }

    fn peek(&self) -> Option<&Token<'src>> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn advance(&mut self) -> Option<&Token<'src>> {
        let token = self.tokens.get(self.pos).map(|(t, _)| t);
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn current_span(&self) -> std::ops::Range<usize> {
        self.tokens
            .get(self.pos)
            .map(|(_, s)| s.clone())
            .unwrap_or(self.input.len()..self.input.len())
    }

    fn expect(&mut self, expected: Token<'src>) -> Result<(), ParseError> {
        let span = self.current_span();
        match self.advance() {
            Some(t) if std::mem::discriminant(t) == std::mem::discriminant(&expected) => Ok(()),
            Some(t) => Err(ParseError {
                message: format!("expected {:?}, got {:?}", expected, t),
                span,
            }),
            None => Err(ParseError {
                message: format!("expected {:?}, got EOF", expected),
                span,
            }),
        }
    }

    fn parse_type_key(&mut self) -> Result<TypeKey<'src>, ParseError> {
        let span = self.current_span();
        match self.peek() {
            Some(Token::Node) => {
                self.advance();
                Ok(TypeKey::Node)
            }
            Some(Token::String) => {
                self.advance();
                Ok(TypeKey::String)
            }
            Some(Token::Invalid) => {
                self.advance();
                Ok(TypeKey::Invalid)
            }
            Some(Token::Unit) => {
                self.advance();
                Ok(TypeKey::Unit)
            }
            Some(Token::UpperIdent(name)) => {
                let name = *name;
                self.advance();
                Ok(TypeKey::Named(name))
            }
            Some(Token::LAngle) => self.parse_synthetic_key(),
            _ => Err(ParseError {
                message: "expected type key".to_string(),
                span,
            }),
        }
    }

    fn parse_synthetic_key(&mut self) -> Result<TypeKey<'src>, ParseError> {
        self.expect(Token::LAngle)?;
        let mut segments = Vec::new();

        loop {
            let span = self.current_span();
            match self.peek() {
                Some(Token::RAngle) => {
                    self.advance();
                    break;
                }
                Some(Token::UpperIdent(s)) => {
                    let s = *s;
                    self.advance();
                    segments.push(s);
                }
                Some(Token::LowerIdent(s)) => {
                    let s = *s;
                    self.advance();
                    segments.push(s);
                }
                _ => {
                    return Err(ParseError {
                        message: "expected identifier or '>'".to_string(),
                        span,
                    });
                }
            }
        }

        if segments.is_empty() {
            return Err(ParseError {
                message: "synthetic key cannot be empty".to_string(),
                span: self.current_span(),
            });
        }

        Ok(TypeKey::Synthetic(segments))
    }

    fn parse_type_value(&mut self) -> Result<TypeValue<'src>, ParseError> {
        let span = self.current_span();
        match self.peek() {
            Some(Token::LBrace) => self.parse_struct(),
            Some(Token::LBracket) => self.parse_tagged_union(),
            Some(Token::Node) => {
                self.advance();
                self.parse_wrapper_or_bare(TypeKey::Node, TypeValue::Node)
            }
            Some(Token::String) => {
                self.advance();
                self.parse_wrapper_or_bare(TypeKey::String, TypeValue::String)
            }
            Some(Token::Invalid) => {
                self.advance();
                self.parse_wrapper_or_bare(TypeKey::Invalid, TypeValue::Invalid)
            }
            Some(Token::Unit) => {
                self.advance();
                self.parse_wrapper_or_bare(TypeKey::Unit, TypeValue::Unit)
            }
            Some(Token::UpperIdent(_)) | Some(Token::LAngle) => {
                let key = self.parse_type_key()?;
                self.parse_wrapper(key)
            }
            _ => Err(ParseError {
                message: "expected type value".to_string(),
                span,
            }),
        }
    }

    fn parse_wrapper_or_bare(
        &mut self,
        key: TypeKey<'src>,
        bare: TypeValue<'src>,
    ) -> Result<TypeValue<'src>, ParseError> {
        match self.peek() {
            Some(Token::Question) => {
                self.advance();
                Ok(TypeValue::Optional(key))
            }
            Some(Token::Star) => {
                self.advance();
                Ok(TypeValue::List(key))
            }
            Some(Token::Plus) => {
                self.advance();
                Ok(TypeValue::NonEmptyList(key))
            }
            _ => Ok(bare),
        }
    }

    fn parse_struct(&mut self) -> Result<TypeValue<'src>, ParseError> {
        self.expect(Token::LBrace)?;
        let mut fields = IndexMap::new();

        loop {
            if matches!(self.peek(), Some(Token::RBrace)) {
                self.advance();
                break;
            }

            let type_key = self.parse_type_key()?;
            self.expect(Token::At)?;

            let span = self.current_span();
            let field_name = match self.advance() {
                Some(Token::LowerIdent(name)) => *name,
                _ => {
                    return Err(ParseError {
                        message: "expected field name (lowercase)".to_string(),
                        span,
                    });
                }
            };

            fields.insert(field_name, type_key);
        }

        Ok(TypeValue::Struct(fields))
    }

    fn parse_tagged_union(&mut self) -> Result<TypeValue<'src>, ParseError> {
        self.expect(Token::LBracket)?;
        let mut variants = IndexMap::new();

        loop {
            if matches!(self.peek(), Some(Token::RBracket)) {
                self.advance();
                break;
            }

            let span = self.current_span();
            let tag = match self.advance() {
                Some(Token::UpperIdent(name)) => *name,
                _ => {
                    return Err(ParseError {
                        message: "expected variant tag (uppercase)".to_string(),
                        span,
                    });
                }
            };

            self.expect(Token::Colon)?;
            let type_key = self.parse_type_key()?;
            variants.insert(tag, type_key);
        }

        Ok(TypeValue::TaggedUnion(variants))
    }

    fn parse_wrapper(&mut self, inner: TypeKey<'src>) -> Result<TypeValue<'src>, ParseError> {
        match self.peek() {
            Some(Token::Question) => {
                self.advance();
                Ok(TypeValue::Optional(inner))
            }
            Some(Token::Star) => {
                self.advance();
                Ok(TypeValue::List(inner))
            }
            Some(Token::Plus) => {
                self.advance();
                Ok(TypeValue::NonEmptyList(inner))
            }
            _ => Err(ParseError {
                message: "expected quantifier (?, *, +) after type key".to_string(),
                span: self.current_span(),
            }),
        }
    }

    fn parse_definition(&mut self) -> Result<(TypeKey<'src>, TypeValue<'src>), ParseError> {
        let span = self.current_span();
        let key = match self.peek() {
            Some(Token::UpperIdent(name)) => {
                let name = *name;
                self.advance();
                TypeKey::Named(name)
            }
            Some(Token::LAngle) => self.parse_synthetic_key()?,
            _ => {
                return Err(ParseError {
                    message: "expected type name (uppercase) or synthetic key".to_string(),
                    span,
                });
            }
        };

        self.expect(Token::Eq)?;
        let value = self.parse_type_value()?;

        Ok((key, value))
    }

    fn parse_all(&mut self) -> Result<TypeTable<'src>, ParseError> {
        let mut table = TypeTable::new();

        while self.peek().is_some() {
            let (key, value) = self.parse_definition()?;
            table.insert(key, value);
        }

        Ok(table)
    }
}

/// Parse tyton notation into a TypeTable.
pub fn parse(input: &str) -> Result<TypeTable<'_>, ParseError> {
    let mut parser = Parser::new(input)?;
    parser.parse_all()
}

/// Emit TypeTable as tyton notation.
pub fn emit(table: &TypeTable<'_>) -> String {
    let mut out = String::new();

    for (key, value) in table.iter() {
        if is_builtin(key) {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        emit_key(&mut out, key);
        out.push_str(" = ");
        emit_value(&mut out, value);
    }

    out
}

fn is_builtin(key: &TypeKey<'_>) -> bool {
    matches!(
        key,
        TypeKey::Node | TypeKey::String | TypeKey::Unit | TypeKey::Invalid
    )
}

fn emit_key(out: &mut String, key: &TypeKey<'_>) {
    match key {
        TypeKey::Node => out.push_str("#Node"),
        TypeKey::String => out.push_str("#string"),
        TypeKey::Invalid => out.push_str("#Invalid"),
        TypeKey::Unit => out.push_str("()"),
        TypeKey::DefaultQuery => out.push_str("#DefaultQuery"),
        TypeKey::Named(name) => out.push_str(name),
        TypeKey::Synthetic(segments) => {
            out.push('<');
            for (i, seg) in segments.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(seg);
            }
            out.push('>');
        }
    }
}

fn emit_value(out: &mut String, value: &TypeValue<'_>) {
    match value {
        TypeValue::Node => out.push_str("#Node"),
        TypeValue::String => out.push_str("#string"),
        TypeValue::Invalid => out.push_str("#Invalid"),
        TypeValue::Unit => out.push_str("()"),
        TypeValue::Struct(fields) => {
            out.push_str("{ ");
            for (i, (field, key)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                emit_key(out, key);
                write!(out, " @{}", field).unwrap();
            }
            out.push_str(" }");
        }
        TypeValue::TaggedUnion(variants) => {
            out.push_str("[ ");
            for (i, (tag, key)) in variants.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write!(out, "{}: ", tag).unwrap();
                emit_key(out, key);
            }
            out.push_str(" ]");
        }
        TypeValue::Optional(key) => {
            emit_key(out, key);
            out.push('?');
        }
        TypeValue::List(key) => {
            emit_key(out, key);
            out.push('*');
        }
        TypeValue::NonEmptyList(key) => {
            emit_key(out, key);
            out.push('+');
        }
    }
}
