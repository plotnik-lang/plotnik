use rowan::TextRange;

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::{PATTERN_FIRST_TOKENS, QUANTIFIERS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuffixMode {
    Apply,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternSlot {
    DefBody,
    AlternativeBody,
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

        self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
            report
                .detail("expected a pattern")
                .hint(
                    "start with `(` for a node, `[` for an alternation, `{` for a sequence, a quote for a literal token, or `_` for a wildcard",
                )
        });
        false
    }

    pub(crate) fn parse_required_pattern(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Apply, PatternSlot::DefBody)
    }

    pub(crate) fn parse_alternative_body(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Apply, PatternSlot::AlternativeBody)
    }

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

        if let Some(report) = self.report_current(DiagnosticKind::ExpectedExpression) {
            report.emit();
        }
    }

    pub(crate) fn parse_rejected_positional(&mut self, kind: DiagnosticKind) {
        let start = self.current_span().start();
        self.parse_pattern();
        let end = self
            .last_non_trivia_end()
            .map_or(start, |end| end.max(start));

        self.diagnostics
            .report(kind, Span::new(self.source_id, TextRange::new(start, end)))
            .emit();
    }

    pub(crate) fn parse_pattern(&mut self) {
        self.parse_pattern_inner(SuffixMode::Apply)
    }

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
        let pattern_start = self.current_span().start();

        let kind = self.current();
        match kind {
            SyntaxKind::ParenOpen => self.parse_named_node(),
            SyntaxKind::BracketOpen => self.parse_alternation(),
            SyntaxKind::BraceOpen => self.parse_sequence(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.parse_string_pattern(),
            SyntaxKind::UnterminatedString => {
                if let Some(report) = self.report_current(DiagnosticKind::UnclosedString) {
                    report
                        .hint("close the quote to finish this literal-token pattern")
                        .emit();
                }
                self.bump_as_error();
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
            self.reject_constraint_suffixes(
                DiagnosticKind::QuantifiedAnchor,
                DiagnosticKind::CapturedAnchor,
            );
        } else if matches!(kind, SyntaxKind::Minus | SyntaxKind::Negation) {
            self.reject_constraint_suffixes(
                DiagnosticKind::QuantifiedNegatedField,
                DiagnosticKind::CapturedNegatedField,
            );
        } else if suffix == SuffixMode::Apply {
            let quantifier = self.try_parse_quantifier(checkpoint);
            let capture = self.try_parse_capture(checkpoint);
            self.reject_trailing_quantifier(pattern_start, quantifier, capture);
        }

        self.exit_recursion();
    }

    fn reject_trailing_quantifier(
        &mut self,
        pattern_start: rowan::TextSize,
        quantifier: Option<TextRange>,
        capture: Option<TextRange>,
    ) {
        if !self.at_ts(QUANTIFIERS) {
            return;
        }

        let trailing = self.current_text().to_string();
        let (detail, hint, remove) = match (quantifier, capture) {
            (Some(first), _) => {
                let first = &self.source[usize::from(first.start())..usize::from(first.end())];
                (
                    format!("this pattern already uses `{first}` and cannot also use `{trailing}`"),
                    None,
                    true,
                )
            }
            (None, Some(capture)) => {
                let body = &self.source[usize::from(pattern_start)..usize::from(capture.start())];
                let capture =
                    &self.source[usize::from(capture.start())..usize::from(capture.end())];
                (
                    format!(
                        "`{trailing}` applies to the pattern, so it must appear before `{capture}`"
                    ),
                    Some(format!("write `{}{trailing} {capture}`", body.trim_end())),
                    false,
                )
            }
            (None, None) => unreachable!("trailing quantifier follows a parsed suffix"),
        };

        self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
            let report = report.detail(detail);
            if remove {
                return report.fix("remove the second quantifier", "");
            }
            report.hint(hint.expect("misordered quantifier has a rewrite"))
        });
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
