//! Core parser state machine and low-level operations.
//!
//! This module contains the `Parser` struct and all foundational methods:
//! - Token access and lookahead
//! - Trivia buffering and attachment
//! - Tree construction via Rowan
//! - Error recording and recovery
//! - Recursion depth limiting

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use super::error::SyntaxError;
use super::MAX_DEPTH;

#[cfg(debug_assertions)]
const DEFAULT_FUEL: u32 = 256;
use crate::ql::lexer::{token_text, Token};
use crate::ql::syntax_kind::{SyntaxKind, TokenSet};

/// Parse result containing the green tree and any errors.
///
/// The tree is always complete—errors are recorded separately and also
/// represented as `SyntaxKind::Error` nodes in the tree itself.
#[derive(Debug, Clone)]
pub struct Parse {
    pub(super) green: GreenNode,
    pub(super) errors: Vec<SyntaxError>,
}

/// Parser state machine.
///
/// The token stream is processed left-to-right. Trivia tokens (whitespace, comments)
/// are buffered separately and flushed as leading trivia when starting a new node.
/// This gives predictable trivia attachment without backtracking.
pub struct Parser<'src> {
    pub(super) source: &'src str,
    pub(super) tokens: Vec<Token>,
    /// Current position in `tokens`. Monotonically increases.
    pub(super) pos: usize,
    /// Trivia accumulated since last non-trivia token.
    /// Drained into tree at `start_node()` / `checkpoint()`.
    pub(super) trivia_buffer: Vec<Token>,
    pub(super) builder: GreenNodeBuilder<'static>,
    pub(super) errors: Vec<SyntaxError>,
    pub(super) depth: u32,
    /// Last error position - used to suppress cascading errors at same span
    pub(super) last_error_pos: Option<TextSize>,
    #[cfg(debug_assertions)]
    pub(super) fuel: std::cell::Cell<u32>,
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
            last_error_pos: None,
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
    pub(super) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Lookahead by `n` tokens (0 = current). Consumes fuel in debug mode.
    pub(super) fn nth(&self, lookahead: usize) -> SyntaxKind {
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

    pub(super) fn current_span(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or_else(|| TextRange::empty(self.eof_offset()), |t| t.span)
    }

    pub(super) fn eof_offset(&self) -> TextSize {
        TextSize::from(self.source.len() as u32)
    }

    pub(super) fn eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub(super) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(super) fn at_set(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    /// Peek past trivia. Buffers trivia tokens for later attachment.
    pub(super) fn peek(&mut self) -> SyntaxKind {
        self.skip_trivia_to_buffer();
        self.current()
    }

    /// Lookahead `n` non-trivia tokens. Used for LL(k) decisions like `field:`.
    pub(super) fn peek_nth(&mut self, n: usize) -> SyntaxKind {
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

    pub(super) fn skip_trivia_to_buffer(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            self.trivia_buffer.push(self.tokens[self.pos]);
            self.pos += 1;
        }
    }

    pub(super) fn drain_trivia(&mut self) {
        for token in self.trivia_buffer.drain(..) {
            let text = token_text(self.source, &token);
            self.builder.token(token.kind.into(), text);
        }
    }

    pub(super) fn eat_trivia(&mut self) {
        self.skip_trivia_to_buffer();
        self.drain_trivia();
    }

    // =========================================================================
    // Tree construction
    // =========================================================================

    /// Start node, attaching any buffered trivia first.
    pub(super) fn start_node(&mut self, kind: SyntaxKind) {
        self.drain_trivia();
        self.builder.start_node(kind.into());
    }

    /// Wrap previously-parsed content. Used for quantifiers: parse `(foo)`, then
    /// see `*`, wrap retroactively into `Quantifier(NamedNode(...), Star)`.
    pub(super) fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind.into());
    }

    pub(super) fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Checkpoint before parsing. If we later need to wrap, use `start_node_at`.
    pub(super) fn checkpoint(&mut self) -> Checkpoint {
        self.drain_trivia();
        self.builder.checkpoint()
    }

    /// Consume current token into tree. Resets fuel.
    pub(super) fn bump(&mut self) {
        assert!(!self.eof(), "bump called at EOF");
        #[cfg(debug_assertions)]
        self.fuel.set(DEFAULT_FUEL);
        let token = self.tokens[self.pos];
        let text = token_text(self.source, &token);
        self.builder.token(token.kind.into(), text);
        self.pos += 1;
    }

    pub(super) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect token. On mismatch: emit error but don't consume (allows parent recovery).
    pub(super) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error_expected(&[kind]);
        false
    }

    // =========================================================================
    // Error handling & recovery
    // =========================================================================

    pub(super) fn error(&mut self, message: impl Into<String>) {
        let range = self.current_span();
        let pos = range.start();
        if self.last_error_pos == Some(pos) {
            return;
        }
        self.last_error_pos = Some(pos);
        self.errors.push(SyntaxError::new(range, message));
    }

    pub(super) fn error_expected(&mut self, expected: &[SyntaxKind]) {
        let msg = if expected.len() == 1 {
            format!("expected {}", expected[0].human_name())
        } else {
            let names: Vec<_> = expected.iter().map(|k| k.human_name()).collect();
            format!("expected one of: {}", names.join(", "))
        };
        self.error(msg);
    }

    /// Wrap unexpected token in Error node and consume it.
    /// Ensures progress even on garbage input.
    pub(super) fn error_and_bump(&mut self, message: &str) {
        self.error(message);
        if !self.eof() {
            self.start_node(SyntaxKind::Error);
            self.bump();
            self.finish_node();
        }
    }

    /// Skip tokens until we hit a recovery point. Wraps skipped tokens in Error node.
    /// If already at recovery token, just emits error without consuming.
    #[allow(dead_code)] // Used by future grammar rules (named expressions)
    pub(super) fn error_recover(&mut self, message: &str, recovery: TokenSet) {
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

    pub(super) fn enter_recursion(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            self.error("recursion limit exceeded");
            return false;
        }
        self.depth += 1;
        true
    }

    pub(super) fn exit_recursion(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}