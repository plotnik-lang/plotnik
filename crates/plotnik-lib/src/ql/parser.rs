//! Resilient LL parser for the query language.
//!
//! # Architecture
//!
//! This parser produces a lossless concrete syntax tree (CST) via Rowan's green tree builder.
//! Key design decisions borrowed from rust-analyzer, rnix-parser, and taplo:
//!
//! - Zero-copy parsing: tokens carry spans, text sliced only when building tree nodes
//! - Trivia buffering: whitespace/comments collected, then attached as leading trivia
//! - Checkpoint-based wrapping: retroactively wrap nodes for quantifiers `*+?`
//! - Explicit recovery sets: per-production sets determine when to bail vs consume errors
//!
//! # Error Recovery Strategy
//!
//! The parser never fails—it always produces a tree. Recovery follows these rules:
//!
//! 1. Unknown tokens get wrapped in `SyntaxKind::Error` nodes and consumed
//! 2. Missing expected tokens emit an error but don't consume (parent may handle)
//! 3. Recovery sets define "synchronization points" per production
//! 4. On recursion limit, remaining input goes into single Error node
//!
//! # Grammar (EBNF-ish)
//!
//! ```text
//! root       = pattern*
//! pattern    = named_node | alternation | wildcard | anon_node
//!            | capture | anchor | negated_field | field | ident
//! named_node = "(" [node_type] pattern* ")"
//! alternation= "[" pattern* "]"
//! wildcard   = "_"
//! anon_node  = STRING
//! capture    = "@" CAPTURE_NAME
//! anchor     = "."
//! negated_field = "!" IDENT
//! field      = IDENT ":" pattern
//! quantifier = pattern ("*" | "+" | "?" | "*?" | "+?" | "??")
//! ```

#![allow(dead_code)]

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use super::lexer::{Token, lex, token_text};
use super::syntax_kind::{SyntaxKind, SyntaxNode, TokenSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: TextRange,
    pub message: String,
}

impl SyntaxError {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error at {}..{}: {}",
            u32::from(self.range.start()),
            u32::from(self.range.end()),
            self.message
        )
    }
}

impl std::error::Error for SyntaxError {}

/// Parse result containing the green tree and any errors.
///
/// The tree is always complete—errors are recorded separately and also
/// represented as `SyntaxKind::Error` nodes in the tree itself.
#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Creates a typed view over the immutable green tree.
    /// This is cheap—SyntaxNode is a thin wrapper with parent pointers.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Stack depth limit. Tree-sitter queries can nest deeply via `(a (b (c ...)))`.
/// 512 handles any reasonable input while preventing stack overflow on malicious input.
const MAX_DEPTH: u32 = 512;

/// Main entry point. Always succeeds—errors embedded in the returned tree.
pub fn parse(source: &str) -> Parse {
    let tokens = lex(source);
    let mut parser = Parser::new(source, tokens);
    parser.parse_root();
    parser.finish()
}

/// Fuel: debug-mode progress detector. Decremented on lookahead, reset on `bump()`.
/// Catches infinite loops from buggy grammar rules that never consume input.
#[cfg(debug_assertions)]
const DEFAULT_FUEL: u32 = 256;

/// Parser state machine.
///
/// The token stream is processed left-to-right. Trivia tokens (whitespace, comments)
/// are buffered separately and flushed as leading trivia when starting a new node.
/// This gives predictable trivia attachment without backtracking.
pub struct Parser<'src> {
    source: &'src str,
    tokens: Vec<Token>,
    /// Current position in `tokens`. Monotonically increases.
    pos: usize,
    /// Trivia accumulated since last non-trivia token.
    /// Drained into tree at `start_node()` / `checkpoint()`.
    trivia_buffer: Vec<Token>,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
    depth: u32,
    #[cfg(debug_assertions)]
    fuel: std::cell::Cell<u32>,
}

impl<'src> Parser<'src> {
    pub fn new(source: &'src str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            trivia_buffer: Vec::with_capacity(4),
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            depth: 0,
            #[cfg(debug_assertions)]
            fuel: std::cell::Cell::new(DEFAULT_FUEL),
        }
    }

    pub fn finish(mut self) -> Parse {
        self.drain_trivia();
        Parse {
            green: self.builder.finish(),
            errors: self.errors,
        }
    }

    // =========================================================================
    // Token access - raw position based, includes trivia
    // =========================================================================

    /// Current token kind. Returns `Error` at EOF (acts as sentinel).
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead by `n` tokens (0 = current). Consumes fuel in debug mode.
    fn nth(&self, lookahead: usize) -> SyntaxKind {
        #[cfg(debug_assertions)]
        {
            if self.fuel.get() == 0 {
                panic!(
                    "parser is stuck: no progress made in {} iterations",
                    DEFAULT_FUEL
                );
            }
            self.fuel.set(self.fuel.get() - 1);
        }
        self.tokens
            .get(self.pos + lookahead)
            .map_or(SyntaxKind::Error, |t| t.kind)
    }

    fn current_span(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or_else(|| TextRange::empty(self.eof_offset()), |t| t.span)
    }

    fn eof_offset(&self) -> TextSize {
        TextSize::from(self.source.len() as u32)
    }

    fn eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn at_set(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    /// Peek past trivia. Buffers trivia tokens for later attachment.
    fn peek(&mut self) -> SyntaxKind {
        self.skip_trivia_to_buffer();
        self.current()
    }

    /// Lookahead `n` non-trivia tokens. Used for LL(k) decisions like `field:`.
    fn peek_nth(&mut self, n: usize) -> SyntaxKind {
        self.skip_trivia_to_buffer();
        let mut count = 0;
        let mut pos = self.pos;
        while pos < self.tokens.len() {
            let kind = self.tokens[pos].kind;
            if !kind.is_trivia() {
                if count == n {
                    return kind;
                }
                count += 1;
            }
            pos += 1;
        }
        SyntaxKind::Error
    }

    // =========================================================================
    // Trivia handling
    //
    // Strategy: buffer trivia, drain as leading trivia when starting nodes.
    // This means `(  foo)` attaches spaces to `foo`, not to `(`.
    // =========================================================================

    fn skip_trivia_to_buffer(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.trivia_buffer.push(self.tokens[self.pos]);
            self.pos += 1;
        }
    }

    fn drain_trivia(&mut self) {
        for token in self.trivia_buffer.drain(..) {
            let text = token_text(self.source, &token);
            self.builder.token(token.kind.into(), text);
        }
    }

    fn eat_trivia(&mut self) {
        self.skip_trivia_to_buffer();
        self.drain_trivia();
    }

    // =========================================================================
    // Tree construction
    // =========================================================================

    /// Start node, attaching any buffered trivia first.
    fn start_node(&mut self, kind: SyntaxKind) {
        self.drain_trivia();
        self.builder.start_node(kind.into());
    }

    /// Wrap previously-parsed content. Used for quantifiers: parse `(foo)`, then
    /// see `*`, wrap retroactively into `Quantifier(NamedNode(...), Star)`.
    fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind.into());
    }

    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Checkpoint before parsing. If we later need to wrap, use `start_node_at`.
    fn checkpoint(&mut self) -> Checkpoint {
        self.drain_trivia();
        self.builder.checkpoint()
    }

    /// Consume current token into tree. Resets fuel.
    fn bump(&mut self) {
        assert!(!self.eof(), "bump called at EOF");
        #[cfg(debug_assertions)]
        self.fuel.set(DEFAULT_FUEL);
        let token = self.tokens[self.pos];
        let text = token_text(self.source, &token);
        self.builder.token(token.kind.into(), text);
        self.pos += 1;
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect token. On mismatch: emit error but don't consume (allows parent recovery).
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error_expected(&[kind]);
        false
    }

    // =========================================================================
    // Error handling & recovery
    // =========================================================================

    fn error(&mut self, message: impl Into<String>) {
        let range = self.current_span();
        self.errors.push(SyntaxError::new(range, message));
    }

    fn error_expected(&mut self, expected: &[SyntaxKind]) {
        let msg = if expected.len() == 1 {
            format!("expected {:?}", expected[0])
        } else {
            format!("expected one of {:?}", expected)
        };
        self.error(msg);
    }

    /// Wrap unexpected token in Error node and consume it.
    /// Ensures progress even on garbage input.
    fn error_and_bump(&mut self, message: &str) {
        self.error(message);
        if !self.eof() {
            self.start_node(SyntaxKind::Error);
            self.bump();
            self.finish_node();
        }
    }

    /// Skip tokens until we hit a recovery point. Wraps skipped tokens in Error node.
    /// If already at recovery token, just emits error without consuming.
    fn error_recover(&mut self, message: &str, recovery: TokenSet) {
        if self.at_set(recovery) || self.eof() {
            self.error(message);
            return;
        }

        self.start_node(SyntaxKind::Error);
        self.error(message);
        while !self.at_set(recovery) && !self.eof() {
            self.bump();
        }
        self.finish_node();
    }

    // =========================================================================
    // Recursion guard
    // =========================================================================

    fn enter_recursion(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.error("recursion limit exceeded");
            return false;
        }
        self.depth += 1;
        true
    }

    fn exit_recursion(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    // =========================================================================
    // Grammar productions
    // =========================================================================

    fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        while self.peek() != SyntaxKind::Error || !self.eof() {
            if self.eof() {
                break;
            }
            self.parse_pattern_or_error();
        }

        self.eat_trivia();
        self.finish_node();
    }

    fn parse_pattern_or_error(&mut self) {
        use super::syntax_kind::token_sets::PATTERN_FIRST;

        let kind = self.peek();
        if PATTERN_FIRST.contains(kind) {
            self.parse_pattern();
        } else {
            self.error_and_bump("expected pattern");
        }
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier suffix.
    fn parse_pattern(&mut self) {
        if !self.enter_recursion() {
            // On limit: consume everything as error, prevent infinite recursion
            self.start_node(SyntaxKind::Error);
            while !self.eof() {
                self.bump();
            }
            self.finish_node();
            return;
        }

        // Checkpoint before the pattern for potential quantifier wrapping
        let checkpoint = self.checkpoint();

        match self.peek() {
            SyntaxKind::ParenOpen => self.parse_named_node(),
            SyntaxKind::BracketOpen => self.parse_alternation(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::StringLit => self.parse_anonymous_node(),
            SyntaxKind::At => self.parse_capture(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation => self.parse_negated_field(),
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => {
                self.parse_node_or_field()
            }
            _ => {
                self.error_and_bump("expected pattern");
            }
        }

        self.try_parse_quantifier(checkpoint);

        self.exit_recursion();
    }

    /// Named node: `(type child1 child2 ...)` or `(_ child1 ...)` for any node.
    fn parse_named_node(&mut self) {
        use super::syntax_kind::token_sets::NAMED_NODE_RECOVERY;

        self.start_node(SyntaxKind::NamedNode);
        self.expect(SyntaxKind::ParenOpen);

        if self.peek() == SyntaxKind::ParenClose {
            self.error("empty node pattern - expected node type or children");
            self.expect(SyntaxKind::ParenClose);
            self.finish_node();
            return;
        }

        // Optional type constraint: `(identifier ...)` or `(_ ...)` for wildcard
        if matches!(
            self.peek(),
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent | SyntaxKind::Underscore
        ) {
            self.bump();
        }

        self.parse_node_children(SyntaxKind::ParenClose, NAMED_NODE_RECOVERY);

        self.expect(SyntaxKind::ParenClose);
        self.finish_node();
    }

    /// Parse children until `until` token or recovery set hit.
    /// Recovery set lets parent handle mismatched delimiters gracefully.
    fn parse_node_children(&mut self, until: SyntaxKind, recovery: TokenSet) {
        use super::syntax_kind::token_sets::PATTERN_FIRST;

        while !self.eof() {
            let kind = self.peek();
            if kind == until {
                break;
            }
            if PATTERN_FIRST.contains(kind) {
                self.parse_pattern();
            } else if recovery.contains(kind) {
                break;
            } else {
                self.error_and_bump("expected pattern or closing delimiter");
            }
        }
    }

    /// Alternation/choice: `[pattern1 pattern2 ...]`
    fn parse_alternation(&mut self) {
        use super::syntax_kind::token_sets::ALTERNATION_RECOVERY;

        self.start_node(SyntaxKind::Alternation);
        self.expect(SyntaxKind::BracketOpen);

        self.parse_node_children(SyntaxKind::BracketClose, ALTERNATION_RECOVERY);

        self.expect(SyntaxKind::BracketClose);
        self.finish_node();
    }

    fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore);
        self.finish_node();
    }

    /// Anonymous (literal) node: `"if"`, `"+"`, etc.
    fn parse_anonymous_node(&mut self) {
        self.start_node(SyntaxKind::AnonNode);
        self.expect(SyntaxKind::StringLit);
        self.finish_node();
    }

    /// Capture binding: `@name` or `@name.field.subfield`
    fn parse_capture(&mut self) {
        self.start_node(SyntaxKind::Capture);
        self.expect(SyntaxKind::At);
        if self.peek() == SyntaxKind::CaptureName {
            self.bump();
        } else {
            self.error("expected capture name");
        }
        self.finish_node();
    }

    /// Anchor for anonymous nodes: `.`
    fn parse_anchor(&mut self) {
        self.start_node(SyntaxKind::Anchor);
        self.expect(SyntaxKind::Dot);
        self.finish_node();
    }

    /// Negated field assertion: `!field` (field must be absent)
    fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation);
        if matches!(self.peek(), SyntaxKind::LowerIdent) {
            self.bump();
        } else {
            self.error("expected field name");
        }
        self.finish_node();
    }

    /// Disambiguate `field: pattern` from bare identifier via LL(2) lookahead.
    fn parse_node_or_field(&mut self) {
        if self.peek_nth(1) == SyntaxKind::Colon {
            self.parse_field();
        } else {
            self.start_node(SyntaxKind::Pattern);
            self.bump();
            self.finish_node();
        }
    }

    /// Field constraint: `field_name: pattern`
    fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        if matches!(self.peek(), SyntaxKind::LowerIdent | SyntaxKind::UpperIdent) {
            self.bump();
        } else {
            self.error("expected field name");
        }

        self.expect(SyntaxKind::Colon);

        self.parse_pattern();

        self.finish_node();
    }

    /// If current token is quantifier, wrap preceding pattern using checkpoint.
    fn try_parse_quantifier(&mut self, checkpoint: Checkpoint) {
        use super::syntax_kind::token_sets::QUANTIFIERS;

        if self.at_set(QUANTIFIERS) {
            self.start_node_at(checkpoint, SyntaxKind::Quantifier);
            self.bump();
            self.finish_node();
        }
    }
}