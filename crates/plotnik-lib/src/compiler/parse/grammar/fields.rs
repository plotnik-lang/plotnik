use rowan::{Checkpoint, TextRange, TextSize};

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::QUANTIFIERS;

use super::utils::starts_uppercase;
use super::validation::Ident;

impl Parser<'_, '_> {
    /// Capture type: `::Type`, `::text`, or `::bool`.
    pub(crate) fn parse_capture_type(&mut self) {
        self.start_node(SyntaxKind::CaptureType);
        self.expect(SyntaxKind::DoubleColon, "'::' for capture type");

        if self.at(SyntaxKind::Id) {
            let ident = self.bump_ident();
            self.validate_capture_type_name(ident);
            self.finish_node();
            return;
        }

        if let Some(report) = self.report_current(DiagnosticKind::ExpectedCaptureType) {
            report.emit();
        }

        self.finish_node();
    }

    /// Handle a single-colon capture type (`@x : Type` instead of `@x :: Type`).
    pub(crate) fn parse_capture_type_single_colon(&mut self) {
        if !self.next_is(SyntaxKind::Id) {
            return;
        }

        self.start_node(SyntaxKind::CaptureType);

        let span = self.current_span();
        if let Some(report) = self.report_at(DiagnosticKind::InvalidCaptureTypeSyntax, span) {
            report.fix("use `::`", "::").emit();
        }

        self.bump();

        // `at` skips trivia, so this handles `@x : Type` with space
        if self.at(SyntaxKind::Id) {
            self.bump();
        }

        self.finish_node();
    }

    /// Negated field assertion: `-field` (field must be absent)
    ///
    /// Also accepts deprecated `!field` syntax with a warning.
    pub(crate) fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);

        if self.at(SyntaxKind::Negation) {
            let span = self.current_span();
            if let Some(report) = self.report_at(DiagnosticKind::NegationSyntaxDeprecated, span) {
                report.fix("use `-`", "-").emit();
            }
            self.bump();
        } else {
            self.expect(SyntaxKind::Minus, "'-' for negated field");
        }

        if !self.at(SyntaxKind::Id) {
            if let Some(report) = self.report_current(DiagnosticKind::ExpectedGrammarFieldName) {
                report.emit();
            }
            self.finish_node();
            return;
        }

        let ident = self.bump_ident();
        self.validate_field_name(ident);
        self.finish_node();
    }

    /// Disambiguate `field: pattern` from bare identifier via LL(2) lookahead.
    /// Also handles `field = pattern` typo (should be `field: pattern`).
    pub(crate) fn parse_id_or_field(&mut self) {
        if self.next_is(SyntaxKind::Colon) {
            self.parse_field();
            return;
        }

        if self.next_is(SyntaxKind::Equals) {
            self.parse_field_equals_typo();
            return;
        }

        // Bare identifiers are not valid patterns; patterns require parentheses
        let span = self.current_span();
        let text = self.current_text();
        let replacement = format!("({text})");
        let is_ref = starts_uppercase(text);
        if let Some(mut report) = self.report_at(DiagnosticKind::BareIdentifier, span) {
            report = report.fix("wrap in parentheses", replacement);
            if is_ref {
                report = report.detail("references must be parenthesized");
            }
            report.emit();
        }
        self.bump_as_error();
    }

    /// Field constraint: `field_name: pattern`
    pub(crate) fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.assert_current(SyntaxKind::Id);
        let ident = self.bump_ident();
        self.validate_field_name(ident);

        self.expect(
            SyntaxKind::Colon,
            "':' to separate field name from its value",
        );

        self.parse_required_pattern_no_suffix();

        self.finish_node();
    }

    /// Handle `field = pattern` typo - parse as Field but emit error.
    pub(crate) fn parse_field_equals_typo(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.bump();
        let span = self.current_span();
        if let Some(report) = self.report_at(DiagnosticKind::InvalidGrammarFieldEquals, span) {
            report.fix("use `:`", ":").emit();
        }
        self.bump();

        self.parse_required_pattern_no_suffix();

        self.finish_node();
    }

    pub(crate) fn try_parse_quantifier(&mut self, checkpoint: Checkpoint) {
        if self.at_ts(QUANTIFIERS) {
            self.start_node_at(checkpoint, SyntaxKind::Quantifier);
            self.bump();
            self.finish_node();
        }
    }

    pub(crate) fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
        let is_capture = self.at(SyntaxKind::CaptureToken);
        let is_discard = self.at(SyntaxKind::DiscardToken);

        if !is_capture && !is_discard {
            return;
        }

        self.start_node_at(checkpoint, SyntaxKind::CapturedPattern);
        self.drain_trivia();
        self.start_node(SyntaxKind::Capture);

        let source = self.source;
        let span = self.current_span();
        self.bump();

        let end = self.consume_dotted_capture_tail(span.end());
        let full_span = TextRange::new(span.start(), end);
        let name = &source[usize::from(span.start()) + 1..usize::from(end)]; // strip @ prefix
        self.validate_capture_name(Ident::new(name, full_span));

        // Capture types are only valid on regular captures.
        if is_capture {
            if self.at(SyntaxKind::DoubleColon) {
                self.parse_capture_type();
            } else if self.at(SyntaxKind::Colon) {
                self.parse_capture_type_single_colon();
            }
        }

        self.finish_node();
        self.finish_node();
    }

    /// Tree-sitter style captures (`@foo.bar`, `@foo-bar`) lex as `@foo` `.`/`-` `bar`.
    /// Consume directly adjacent `.ident`/`-ident` runs into an Error node so the
    /// whole thing is diagnosed as one malformed capture name, not as an anchor or
    /// negated field followed by a bare identifier. Returns the capture's end offset.
    fn consume_dotted_capture_tail(&mut self, mut end: TextSize) -> TextSize {
        let dotted_segment = |p: &Self, end: TextSize| {
            let sep = p
                .tokens
                .get(p.pos)
                .filter(|t| matches!(t.kind, SyntaxKind::Dot | SyntaxKind::Minus))?;
            if sep.span.start() != end {
                return None;
            }
            let id = p
                .tokens
                .get(p.pos + 1)
                .filter(|t| t.kind == SyntaxKind::Id)?;
            (id.span.start() == sep.span.end()).then_some(id.span.end())
        };

        if dotted_segment(self, end).is_none() {
            return end;
        }

        self.start_node(SyntaxKind::Error);
        while let Some(segment_end) = dotted_segment(self, end) {
            self.bump(); // separator
            self.bump(); // identifier
            end = segment_end;
        }
        self.finish_node();
        end
    }
}
