//! Parser state machine and low-level operations.

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use super::ast::Root;
use super::cst::{SyntaxKind, SyntaxNode, TokenSet};
use super::lexer::{Token, token_text};
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::Error;

#[derive(Debug)]
pub struct ParseResult {
    ast: Root,
    fuel_consumed: u32,
}

impl ParseResult {
    pub fn into_ast(self) -> Root {
        self.ast
    }

    pub fn fuel_consumed(&self) -> u32 {
        self.fuel_consumed
    }
}

/// Span of the opening token of an unclosed-so-far delimiter pair.
#[derive(Debug, Clone, Copy)]
struct OpenDelimiter {
    span: TextRange,
}

/// Default parsing fuel limit.
pub const DEFAULT_FUEL: u32 = 1_000_000;
/// Default maximum recursion depth.
pub const DEFAULT_MAX_DEPTH: u32 = 4096;

/// Resource limits for a parse run.
#[derive(Debug, Clone, Copy)]
pub struct ParseConfig {
    pub fuel: u32,
    pub max_depth: u32,
}
/// Lookaheads allowed without consuming a token before the stuck-parser assertion fires.
const MAX_STALL_LOOKAHEADS: u32 = 256;

/// Trivia tokens are buffered and flushed when starting a new node.
pub struct Parser<'q, 'd> {
    pub(super) source: &'q str,
    pub(super) source_id: SourceId,
    pub(super) tokens: Vec<Token>,
    pub(super) pos: usize,
    pub(super) pending_trivia: Vec<Token>,
    pub(super) builder: GreenNodeBuilder<'static>,
    pub(super) diagnostics: &'d mut Diagnostics,
    pub(super) depth: u32,
    pub(super) last_diagnostic_pos: Option<TextSize>,
    delimiter_stack: Vec<OpenDelimiter>,
    pub(super) stall_guard: std::cell::Cell<u32>,
    pub(crate) fuel_initial: u32,
    pub(crate) fuel_remaining: u32,
    pub(crate) max_depth: u32,
    pub(crate) fatal_error: Option<Error>,
}

impl<'q, 'd> Parser<'q, 'd> {
    /// Create a new parser with the specified parameters.
    pub fn new(
        source: &'q str,
        source_id: SourceId,
        tokens: Vec<Token>,
        diagnostics: &'d mut Diagnostics,
        config: ParseConfig,
    ) -> Self {
        Parser {
            source,
            source_id,
            tokens,
            pos: 0,
            pending_trivia: Vec::with_capacity(4),
            builder: GreenNodeBuilder::new(),
            diagnostics,
            depth: 0,
            last_diagnostic_pos: None,
            delimiter_stack: Vec::with_capacity(8),
            stall_guard: std::cell::Cell::new(MAX_STALL_LOOKAHEADS),
            fuel_initial: config.fuel,
            fuel_remaining: config.fuel,
            max_depth: config.max_depth,
            fatal_error: None,
        }
    }

    pub fn parse(mut self) -> Result<ParseResult, Error> {
        self.parse_root();
        let (cst, parse_fuel_consumed) = self.finish()?;
        let root = Root::cast(SyntaxNode::new_root(cst)).expect("parser always produces Root");
        Ok(ParseResult {
            ast: root,
            fuel_consumed: parse_fuel_consumed,
        })
    }

    fn finish(mut self) -> Result<(GreenNode, u32), Error> {
        self.drain_trivia();
        if let Some(err) = self.fatal_error {
            return Err(err);
        }
        let fuel_consumed = self.fuel_initial.saturating_sub(self.fuel_remaining);
        Ok((self.builder.finish(), fuel_consumed))
    }

    pub(super) fn has_fatal_error(&self) -> bool {
        self.fatal_error.is_some()
    }

    pub(super) fn current(&mut self) -> SyntaxKind {
        self.skip_trivia_to_buffer();
        self.nth_raw(0)
    }

    fn reset_stall_guard(&self) {
        self.stall_guard.set(MAX_STALL_LOOKAHEADS);
    }

    pub(super) fn nth_raw(&self, lookahead: usize) -> SyntaxKind {
        self.ensure_progress();
        self.tokens
            .get(self.pos + lookahead)
            .map_or(SyntaxKind::Error, |t| t.kind)
    }

    fn consume_parse_fuel(&mut self) {
        if self.fuel_remaining > 0 {
            self.fuel_remaining -= 1;
            return;
        }

        if self.fatal_error.is_none() {
            self.fatal_error = Some(Error::ParseFuelExhausted);
        }
    }

    pub(super) fn current_span(&mut self) -> TextRange {
        self.skip_trivia_to_buffer();
        self.tokens
            .get(self.pos)
            .map_or_else(|| TextRange::empty(self.eof_offset()), |t| t.span)
    }

    /// Text of the current token (empty at EOF). Borrows from the source, not the parser.
    pub(super) fn current_text(&mut self) -> &'q str {
        self.skip_trivia_to_buffer();
        self.tokens
            .get(self.pos)
            .map_or("", |t| token_text(self.source, t))
    }

    pub(super) fn eof_offset(&self) -> TextSize {
        TextSize::from(self.source.len() as u32)
    }

    pub(super) fn eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub(super) fn is_done(&self) -> bool {
        self.eof() || self.has_fatal_error()
    }

    pub(super) fn at(&mut self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(super) fn at_ts(&mut self, set: TokenSet) -> bool {
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
            self.pending_trivia.push(self.tokens[self.pos]);
            self.pos += 1;
        }
    }

    pub(super) fn drain_trivia(&mut self) {
        for token in self.pending_trivia.drain(..) {
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
        self.reset_stall_guard();
        self.consume_parse_fuel();

        self.drain_trivia();

        let token = self.tokens[self.pos];
        let text = token_text(self.source, &token);
        self.builder.token(token.kind.into(), text);
        self.pos += 1;
    }

    pub(super) fn eat_token(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
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
        match self.delimiter_stack.last() {
            Some(d) => TextRange::new(d.span.start(), self.eof_offset()),
            None => self.current_span(),
        }
    }

    fn should_report(&mut self, pos: TextSize) -> bool {
        if self.last_diagnostic_pos == Some(pos) {
            return false;
        }
        self.last_diagnostic_pos = Some(pos);
        true
    }

    pub(super) fn bump_as_error(&mut self) {
        if !self.eof() {
            self.start_node(SyntaxKind::Error);
            self.bump();
            self.finish_node();
        }
    }

    fn error_ranges(&mut self) -> Option<(TextRange, TextRange)> {
        let range = self.current_span();
        if !self.should_report(range.start()) {
            return None;
        }
        let suppression = self.current_suppression_span();
        Some((range, suppression))
    }

    pub(super) fn error(&mut self, kind: DiagnosticKind) {
        let Some((range, suppression)) = self.error_ranges() else {
            return;
        };
        self.diagnostics
            .report(self.source_id, kind, range)
            .suppression_range(suppression)
            .emit();
    }

    pub(super) fn error_msg(&mut self, kind: DiagnosticKind, message: impl Into<String>) {
        let Some((range, suppression)) = self.error_ranges() else {
            return;
        };
        self.diagnostics
            .report(self.source_id, kind, range)
            .detail(message)
            .suppression_range(suppression)
            .emit();
    }

    pub(super) fn error_with_hint(&mut self, kind: DiagnosticKind, hint: impl Into<String>) {
        let Some((range, suppression)) = self.error_ranges() else {
            return;
        };
        self.diagnostics
            .report(self.source_id, kind, range)
            .hint(hint)
            .suppression_range(suppression)
            .emit();
    }

    /// Emit a diagnostic over an explicit range, deduped by start like `error`.
    /// For diagnostics whose span covers a run already consumed by the caller.
    pub(super) fn error_at(&mut self, kind: DiagnosticKind, range: TextRange) {
        if !self.should_report(range.start()) {
            return;
        }
        let suppression = self.current_suppression_span();
        self.diagnostics
            .report(self.source_id, kind, range)
            .suppression_range(suppression)
            .emit();
    }

    /// Like [`Self::error_at`], but with an explicit hint overriding the diagnostic default.
    pub(super) fn error_at_with_hint(
        &mut self,
        kind: DiagnosticKind,
        range: TextRange,
        hint: impl Into<String>,
    ) {
        if !self.should_report(range.start()) {
            return;
        }
        let suppression = self.current_suppression_span();
        self.diagnostics
            .report(self.source_id, kind, range)
            .hint(hint)
            .suppression_range(suppression)
            .emit();
    }

    pub(super) fn error_and_bump(&mut self, kind: DiagnosticKind) {
        self.error(kind);
        self.bump_as_error();
    }

    pub(super) fn error_and_bump_with_hint(
        &mut self,
        kind: DiagnosticKind,
        hint: impl Into<String>,
    ) {
        self.error_with_hint(kind, hint);
        self.bump_as_error();
    }

    /// Like [`Self::error_and_bump`], but attaches a machine-applicable fix over the
    /// offending token before consuming it.
    pub(super) fn error_and_bump_with_fix(
        &mut self,
        kind: DiagnosticKind,
        fix_description: impl Into<String>,
        fix_replacement: impl Into<String>,
    ) {
        if let Some((range, suppression)) = self.error_ranges() {
            self.diagnostics
                .report(self.source_id, kind, range)
                .suppression_range(suppression)
                .fix(fix_description, fix_replacement)
                .emit();
        }
        self.bump_as_error();
    }

    pub(super) fn enter_recursion(&mut self) -> bool {
        if self.depth < self.max_depth {
            self.depth += 1;
            self.reset_stall_guard();
            return true;
        }

        if self.fatal_error.is_none() {
            self.fatal_error = Some(Error::RecursionLimitExceeded);
        }

        false
    }

    pub(super) fn exit_recursion(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        self.reset_stall_guard();
    }

    pub(super) fn push_delimiter(&mut self) {
        let span = self.current_span();
        self.delimiter_stack.push(OpenDelimiter { span });
    }

    pub(super) fn pop_delimiter(&mut self) {
        self.delimiter_stack.pop();
    }

    /// Report an unclosed delimiter at EOF, pointing back at its opening token.
    pub(super) fn error_unclosed_at_eof(&mut self, kind: DiagnosticKind, construct: &str) {
        let open = self.delimiter_stack.last().copied().unwrap_or_else(|| {
            panic!(
                "unclosed {construct} at EOF but delimiter_stack is empty \
                 (caller must push delimiter before parsing children)"
            )
        });

        let current = self.current_span();
        if !self.should_report(current.start()) {
            return;
        }
        // Use full range for easier downstream error suppression
        let full_range = TextRange::new(open.span.start(), current.end());
        self.diagnostics
            .report(self.source_id, kind, full_range)
            .related_to(
                self.source_id,
                open.span,
                format!("{construct} started here"),
            )
            .emit();
    }

    pub(super) fn last_non_trivia_end(&self) -> Option<TextSize> {
        self.tokens[..self.pos]
            .iter()
            .rev()
            .find(|t| !t.kind.is_trivia())
            .map(|t| t.span.end())
    }

    /// Report a diagnostic with a suggested replacement for `range`.
    /// The replacement must be valid as a verbatim substitute for the range's text.
    pub(super) fn error_with_fix(
        &mut self,
        kind: DiagnosticKind,
        range: TextRange,
        fix_description: impl Into<String>,
        fix_replacement: impl Into<String>,
    ) {
        if !self.should_report(range.start()) {
            return;
        }
        self.diagnostics
            .report(self.source_id, kind, range)
            .fix(fix_description, fix_replacement)
            .emit();
    }
}
