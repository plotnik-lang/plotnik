use rowan::TextRange;

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::{PATTERN_FIRST_TOKENS, QUANTIFIERS};

/// Whether a parsed pattern should absorb a trailing quantifier/capture suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuffixMode {
    Apply,
    Skip,
}

/// Which grammar slot a required pattern fills. Anchors and negated fields
/// are positional assertions, not patterns: they only mean something as a
/// direct child of a node (anchors also in sequences). Every other slot
/// rejects them with a slot-specific diagnostic; the AST cannot even
/// represent them there (`body()`/`value()` would come back empty), so
/// letting them through either crashes lowering or silently drops the
/// written constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternSlot {
    /// `Name = <here>`. Anchors and negated grammar fields parse, then analysis
    /// explains that a definition body must contain a pattern. This is more
    /// useful than rejecting the positional syntax during parsing.
    DefBody,
    /// `[Label: <here> ...]`.
    AlternativeBody,
    /// `field: <here>`.
    FieldValue,
}

impl PatternSlot {
    fn positional_rejection(self, token: SyntaxKind) -> Option<DiagnosticKind> {
        let is_anchor = matches!(token, SyntaxKind::Dot | SyntaxKind::DotBang);
        let is_negated_field = matches!(token, SyntaxKind::Minus | SyntaxKind::Negation);

        match self {
            Self::DefBody => None,
            Self::AlternativeBody if is_anchor => Some(DiagnosticKind::AnchorInAlternation),
            Self::AlternativeBody if is_negated_field => {
                Some(DiagnosticKind::NegatedFieldInAlternation)
            }
            Self::FieldValue if is_anchor => Some(DiagnosticKind::AnchorAsGrammarFieldValue),
            Self::FieldValue if is_negated_field => {
                Some(DiagnosticKind::NegatedFieldAsGrammarFieldValue)
            }
            _ => None,
        }
    }
}

impl Parser<'_, '_> {
    /// Parse a pattern, or emit an error if current token can't start one.
    /// Returns `true` if a valid pattern was parsed, `false` on error.
    pub(crate) fn parse_pattern_or_error(&mut self) -> bool {
        if self.at_ts(PATTERN_FIRST_TOKENS) {
            self.parse_pattern();
            return true;
        }

        if self.at(SyntaxKind::At) {
            self.error_and_bump(DiagnosticKind::CaptureWithoutTarget);
            return false;
        }

        if self.at(SyntaxKind::DoubleColon) {
            self.start_node(SyntaxKind::Error);
            if let Some(report) = self.report_current(DiagnosticKind::CaptureTypeWithoutCapture) {
                report.emit();
            }
            self.parse_capture_type();
            self.finish_node();
            return false;
        }

        if self.at_ts_predicate() {
            self.error_unsupported_predicate();
            return false;
        }

        self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
            report
                .detail("expected a pattern")
                .hint("try `(node)`, `[a b]`, `{a b}`, `\"literal\"`, or `_`")
        });
        false
    }

    /// Parse a pattern required after a prefix like `=`, `field:`, or an alternative label.
    ///
    /// On a non-pattern token this reports `ExpectedExpression` at the current position,
    /// except a misplaced tree-sitter predicate (`#eq?`), which gets its dedicated diagnostic
    /// instead of the generic one — these are exactly the spots a tree-sitter user pastes them.
    pub(crate) fn parse_required_pattern(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Apply, PatternSlot::DefBody)
    }

    /// Like [`Self::parse_required_pattern`], for an alternative body.
    pub(crate) fn parse_alternative_body(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Apply, PatternSlot::AlternativeBody)
    }

    /// Like [`Self::parse_required_pattern`], but without applying a quantifier/capture suffix —
    /// for field values, so the suffix wraps the whole field constraint (see
    /// [`Self::parse_pattern_no_suffix`]).
    pub(crate) fn parse_required_pattern_no_suffix(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Skip, PatternSlot::FieldValue)
    }

    fn parse_required_pattern_inner(&mut self, suffix: SuffixMode, slot: PatternSlot) {
        if let Some(kind) = slot.positional_rejection(self.current()) {
            self.parse_rejected_positional(kind);
            return;
        }

        if self.at_ts(PATTERN_FIRST_TOKENS) {
            match suffix {
                SuffixMode::Apply => self.parse_pattern(),
                SuffixMode::Skip => self.parse_pattern_no_suffix(),
            }
            return;
        }

        if self.at_ts_predicate() {
            self.error_unsupported_predicate();
            return;
        }

        if let Some(report) = self.report_current(DiagnosticKind::ExpectedExpression) {
            report.emit();
        }
    }

    /// Parse an anchor or negated field sitting in a slot that does not admit
    /// one, then report `kind` spanning the whole construct. Parsing it for
    /// real (instead of bumping tokens into an error node) keeps the CST
    /// shape and lets the usual suffix rejection consume any trailing
    /// `*`/`@x` without a cascade of unrelated errors.
    pub(crate) fn parse_rejected_positional(&mut self, kind: DiagnosticKind) {
        let start = self.current_span().start();
        self.parse_pattern();
        let end = self
            .last_non_trivia_end()
            .map_or(start, |end| end.max(start));

        // Reported past `report_at`: the parse above may already have reported
        // at this exact offset (the `!field` deprecation warning), and the
        // parser's one-diagnostic-per-offset dedup would swallow this error —
        // the one that actually gates the pipeline.
        self.diagnostics
            .report(kind, Span::new(self.source_id, TextRange::new(start, end)))
            .emit();
    }

    pub(crate) fn parse_pattern(&mut self) {
        self.parse_pattern_inner(SuffixMode::Apply)
    }

    /// Parse pattern without applying quantifier/capture suffix.
    ///
    /// Used for field values so that suffixes apply to the whole field constraint:
    /// - `field: (x)*` parses as `(field: (x))*` — repeat the field (e.g., decorators)
    /// - `field: (x) @cap` parses as `(field: (x)) @cap` — capture the field pattern
    ///
    /// For captures on structured values (variants/records), compilation handles this
    /// by looking through FieldPattern to determine the actual value type. See
    /// `build_capture_effects` in compile/capture.rs.
    pub(crate) fn parse_pattern_no_suffix(&mut self) {
        self.parse_pattern_inner(SuffixMode::Skip)
    }

    fn parse_pattern_inner(&mut self, suffix: SuffixMode) {
        if !self.enter_recursion() {
            self.start_node(SyntaxKind::Error);
            while !self.is_done() {
                self.bump();
            }
            self.finish_node();
            return;
        }

        let checkpoint = self.checkpoint();

        let kind = self.current();
        match kind {
            SyntaxKind::ParenOpen => self.parse_named_node(),
            SyntaxKind::BracketOpen => self.parse_alternation(),
            SyntaxKind::BraceOpen => self.parse_sequence(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.parse_string_pattern(),
            SyntaxKind::UnterminatedString => {
                self.error_and_bump(DiagnosticKind::UnclosedString);
            }
            SyntaxKind::Dot | SyntaxKind::DotBang => self.parse_anchor(),
            SyntaxKind::Negation | SyntaxKind::Minus => self.parse_negated_field(),
            SyntaxKind::Id => self.parse_id_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(DiagnosticKind::ErrorMissingOutsideParens);
            }
            _ => {
                self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
                    report.detail("expected a pattern")
                });
            }
        }

        if matches!(kind, SyntaxKind::Dot | SyntaxKind::DotBang) {
            // Anchors constrain position and produce no value: `*` or `@x` after
            // one is always a mistake, never a suffix to wrap.
            self.reject_constraint_suffixes(
                DiagnosticKind::QuantifiedAnchor,
                DiagnosticKind::CapturedAnchor,
            );
        } else if matches!(kind, SyntaxKind::Minus | SyntaxKind::Negation) {
            // Negated fields likewise: wrapping one in a quantifier or capture
            // would hide the `NegatedField` node from every consumer (they all
            // look at direct node children) and lower the constraint into
            // nothing.
            self.reject_constraint_suffixes(
                DiagnosticKind::QuantifiedNegatedField,
                DiagnosticKind::CapturedNegatedField,
            );
        } else if suffix == SuffixMode::Apply {
            self.try_parse_quantifier(checkpoint);
            self.try_parse_capture(checkpoint);
        }

        self.exit_recursion();
    }

    fn reject_constraint_suffixes(&mut self, quantified: DiagnosticKind, captured: DiagnosticKind) {
        loop {
            if self.at_ts(QUANTIFIERS) {
                self.report_current_and_bump(quantified, |report| {
                    report.fix("remove the quantifier", "")
                });
            } else if matches!(
                self.current(),
                SyntaxKind::CaptureToken | SyntaxKind::DiscardToken
            ) {
                self.report_current_and_bump(captured, |report| {
                    report.fix("remove the capture", "")
                });
            } else {
                return;
            }
        }
    }
}
