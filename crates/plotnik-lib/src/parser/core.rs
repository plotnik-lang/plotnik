//! Parser state machine and low-level operations.

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use super::ast::Root;
use super::cst::{SyntaxKind, SyntaxNode, TokenSet};
use super::lexer::{Token, token_text};
use crate::Error;
use crate::diagnostics::{DiagnosticKind, Diagnostics};

#[derive(Debug)]
pub struct ParseResult {
    pub root: Root,
    pub diagnostics: Diagnostics,
    pub exec_fuel_consumed: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OpenDelimiter {
    #[allow(dead_code)] // for future mismatch detection
    pub kind: SyntaxKind,
    pub span: TextRange,
}

/// Trivia tokens are buffered and flushed when starting a new node.
pub struct Parser<'src> {
    pub(super) source: &'src str,
    pub(super) tokens: Vec<Token>,
    pub(super) pos: usize,
    pub(super) trivia_buffer: Vec<Token>,
    pub(super) builder: GreenNodeBuilder<'static>,
    pub(super) diagnostics: Diagnostics,
    pub(super) depth: u32,
    pub(super) last_diagnostic_pos: Option<TextSize>,
    pub(super) delimiter_stack: Vec<OpenDelimiter>,
    pub(super) debug_fuel: std::cell::Cell<u32>,
    exec_fuel_initial: Option<u32>,
    exec_fuel_remaining: Option<u32>,
    recursion_fuel_limit: Option<u32>,
    fatal_error: Option<Error>,
}

impl<'src> Parser<'src> {
    pub fn new(source: &'src str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            trivia_buffer: Vec::with_capacity(4),
            builder: GreenNodeBuilder::new(),
            diagnostics: Diagnostics::new(),
            depth: 0,
            last_diagnostic_pos: None,
            delimiter_stack: Vec::with_capacity(8),
            debug_fuel: std::cell::Cell::new(256),
            exec_fuel_initial: None,
            exec_fuel_remaining: None,
            recursion_fuel_limit: None,
            fatal_error: None,
        }
    }

    pub fn with_exec_fuel(mut self, limit: Option<u32>) -> Self {
        self.exec_fuel_initial = limit;
        self.exec_fuel_remaining = limit;
        self
    }

    pub fn with_recursion_fuel(mut self, limit: Option<u32>) -> Self {
        self.recursion_fuel_limit = limit;
        self
    }

    pub fn parse(mut self) -> Result<ParseResult, Error> {
        self.parse_root();
        let (cst, diagnostics, exec_fuel_consumed) = self.finish()?;
        let root = Root::cast(SyntaxNode::new_root(cst)).expect("parser always produces Root");
        Ok(ParseResult {
            root,
            diagnostics,
            exec_fuel_consumed,
        })
    }

    fn finish(mut self) -> Result<(GreenNode, Diagnostics, u32), Error> {
        self.drain_trivia();
        if let Some(err) = self.fatal_error {
            return Err(err);
        }
        let exec_fuel_consumed = match (self.exec_fuel_initial, self.exec_fuel_remaining) {
            (Some(initial), Some(remaining)) => initial.saturating_sub(remaining),
            _ => 0,
        };
        Ok((self.builder.finish(), self.diagnostics, exec_fuel_consumed))
    }

    pub(super) fn has_fatal_error(&self) -> bool {
        self.fatal_error.is_some()
    }

    pub(super) fn current(&mut self) -> SyntaxKind {
        self.skip_trivia_to_buffer();
        self.nth_raw(0)
    }

    fn reset_debug_fuel(&self) {
        self.debug_fuel.set(256);
    }

    pub(super) fn nth_raw(&self, lookahead: usize) -> SyntaxKind {
        self.ensure_progress();
        self.tokens
            .get(self.pos + lookahead)
            .map_or(SyntaxKind::Error, |t| t.kind)
    }

    fn consume_exec_fuel(&mut self) {
        if let Some(ref mut remaining) = self.exec_fuel_remaining {
            if *remaining == 0 {
                if self.fatal_error.is_none() {
                    self.fatal_error = Some(Error::ExecFuelExhausted);
                }
                return;
            }
            *remaining -= 1;
        }
    }

    pub(super) fn current_span(&mut self) -> TextRange {
        self.skip_trivia_to_buffer();
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

    pub(super) fn should_stop(&self) -> bool {
        self.eof() || self.has_fatal_error()
    }

    pub(super) fn currently_is(&mut self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(super) fn currently_is_one_of(&mut self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    /// LL(k) lookahead past trivia.
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

    pub(super) fn next_is(&mut self, kind: SyntaxKind) -> bool {
        self.peek_nth(1) == kind
    }

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

    pub(super) fn start_node(&mut self, kind: SyntaxKind) {
        self.drain_trivia();
        self.builder.start_node(kind.into());
    }

    pub(super) fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind.into());
    }

    pub(super) fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    pub(super) fn checkpoint(&mut self) -> Checkpoint {
        self.drain_trivia();
        self.builder.checkpoint()
    }

    pub(super) fn bump(&mut self) {
        assert!(!self.eof(), "bump called at EOF");
        self.reset_debug_fuel();
        self.consume_exec_fuel();

        self.drain_trivia();

        let token = self.tokens[self.pos];
        let text = token_text(self.source, &token);
        self.builder.token(token.kind.into(), text);
        self.pos += 1;
    }

    pub(super) fn skip_token(&mut self) {
        assert!(!self.eof(), "skip_token called at EOF");
        self.reset_debug_fuel();
        self.consume_exec_fuel();
        self.pos += 1;
    }

    pub(super) fn eat_token(&mut self, kind: SyntaxKind) -> bool {
        if self.currently_is(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// On mismatch: emit diagnostic but don't consume.
    pub(super) fn expect(&mut self, kind: SyntaxKind, what: &str) -> bool {
        if self.eat_token(kind) {
            return true;
        }
        self.error_msg(
            DiagnosticKind::UnexpectedToken,
            format!("expected {}", what),
        );
        false
    }

    pub(super) fn current_suppression_span(&mut self) -> TextRange {
        self.delimiter_stack
            .last()
            .map(|d| TextRange::new(d.span.start(), TextSize::from(self.source.len() as u32)))
            .unwrap_or_else(|| self.current_span())
    }

    fn should_report(&mut self, pos: TextSize) -> bool {
        if self.last_diagnostic_pos == Some(pos) {
            return false;
        }
        self.last_diagnostic_pos = Some(pos);
        true
    }

    fn bump_as_error(&mut self) {
        if !self.eof() {
            self.start_node(SyntaxKind::Error);
            self.bump();
            self.finish_node();
        }
    }

    fn get_error_ranges(&mut self) -> Option<(TextRange, TextRange)> {
        let range = self.current_span();
        if !self.should_report(range.start()) {
            return None;
        }
        let suppression = self.current_suppression_span();
        Some((range, suppression))
    }

    pub(super) fn error(&mut self, kind: DiagnosticKind) {
        let Some((range, suppression)) = self.get_error_ranges() else {
            return;
        };
        self.diagnostics
            .report(kind, range)
            .suppression_range(suppression)
            .emit();
    }

    pub(super) fn error_msg(&mut self, kind: DiagnosticKind, message: impl Into<String>) {
        let Some((range, suppression)) = self.get_error_ranges() else {
            return;
        };
        self.diagnostics
            .report(kind, range)
            .message(message)
            .suppression_range(suppression)
            .emit();
    }

    pub(super) fn error_and_bump(&mut self, kind: DiagnosticKind) {
        self.error(kind);
        self.bump_as_error();
    }

    pub(super) fn error_and_bump_msg(&mut self, kind: DiagnosticKind, message: impl Into<String>) {
        self.error_msg(kind, message);
        self.bump_as_error();
    }

    #[allow(dead_code)]
    pub(super) fn error_recover(
        &mut self,
        kind: DiagnosticKind,
        message: &str,
        recovery: TokenSet,
    ) {
        if self.currently_is_one_of(recovery) || self.should_stop() {
            self.error_msg(kind, message);
            return;
        }

        self.start_node(SyntaxKind::Error);
        self.error_msg(kind, message);
        while !self.currently_is_one_of(recovery) && !self.should_stop() {
            self.bump();
        }
        self.finish_node();
    }

    pub(super) fn enter_recursion(&mut self) -> bool {
        if let Some(limit) = self.recursion_fuel_limit
            && self.depth >= limit
        {
            if self.fatal_error.is_none() {
                self.fatal_error = Some(Error::RecursionLimitExceeded);
            }
            return false;
        }
        self.depth += 1;
        self.reset_debug_fuel();
        true
    }

    pub(super) fn exit_recursion(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        self.reset_debug_fuel();
    }

    pub(super) fn push_delimiter(&mut self, kind: SyntaxKind) {
        let span = self.current_span();
        self.delimiter_stack.push(OpenDelimiter { kind, span });
    }

    pub(super) fn pop_delimiter(&mut self) -> Option<OpenDelimiter> {
        self.delimiter_stack.pop()
    }

    pub(super) fn error_unclosed_delimiter(
        &mut self,
        kind: DiagnosticKind,
        message: impl Into<String>,
        related_msg: impl Into<String>,
        open_range: TextRange,
    ) {
        let current = self.current_span();
        if !self.should_report(current.start()) {
            return;
        }
        // Use full range for easier downstream error suppression
        let full_range = TextRange::new(open_range.start(), current.end());
        self.diagnostics
            .report(kind, full_range)
            .message(message)
            .related_to(related_msg, open_range)
            .emit();
    }

    pub(super) fn last_non_trivia_end(&self) -> Option<TextSize> {
        self.tokens[..self.pos]
            .iter()
            .rev()
            .find(|t| !t.kind.is_trivia())
            .map(|t| t.span.end())
    }

    pub(super) fn error_with_fix(
        &mut self,
        kind: DiagnosticKind,
        range: TextRange,
        message: impl Into<String>,
        fix_description: impl Into<String>,
        fix_replacement: impl Into<String>,
    ) {
        if !self.should_report(range.start()) {
            return;
        }
        self.diagnostics
            .report(kind, range)
            .message(message)
            .fix(fix_description, fix_replacement)
            .emit();
    }
}
