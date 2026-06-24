use rowan::{Checkpoint, TextRange, TextSize};

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::QUANTIFIERS;

use super::utils::starts_uppercase;

impl Parser<'_, '_> {
    /// Type annotation: `::Type` (PascalCase)
    pub(crate) fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon, "'::' for type annotation");

        if self.at(SyntaxKind::Id) {
            let span = self.current_span();
            let text = self.current_text();
            self.bump();
            self.validate_type_name(text, span);
        } else {
            self.error(DiagnosticKind::ExpectedTypeName);
        }

        self.finish_node();
    }

    /// Handle single colon type annotation (common mistake: `@x : Type` instead of `@x :: Type`)
    pub(crate) fn parse_type_annotation_single_colon(&mut self) {
        if !self.next_is(SyntaxKind::Id) {
            return;
        }

        self.start_node(SyntaxKind::Type);

        let span = self.current_span();
        self.error_with_fix(
            DiagnosticKind::InvalidTypeAnnotationSyntax,
            span,
            "use `::`",
            "::",
        );

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
            self.error_with_fix(
                DiagnosticKind::NegationSyntaxDeprecated,
                span,
                "use `-`",
                "-",
            );
            self.bump();
        } else {
            self.expect(SyntaxKind::Minus, "'-' for negated field");
        }

        if !self.at(SyntaxKind::Id) {
            self.error(DiagnosticKind::ExpectedFieldName);
            self.finish_node();
            return;
        }

        let span = self.current_span();
        let text = self.current_text();
        self.bump();
        self.validate_field_name(text, span);
        self.finish_node();
    }

    /// Disambiguate `field: pattern` from bare identifier via LL(2) lookahead.
    /// Also handles `field = pattern` typo (should be `field: pattern`).
    pub(crate) fn parse_tree_or_field(&mut self) {
        if self.next_is(SyntaxKind::Colon) {
            self.parse_field();
            return;
        }

        if self.next_is(SyntaxKind::Equals) {
            self.parse_field_equals_typo();
            return;
        }

        // Bare identifiers are not valid expressions; patterns require parentheses
        let span = self.current_span();
        let text = self.current_text();
        let mut report = self
            .diagnostics
            .report(self.source_id, DiagnosticKind::BareIdentifier, span)
            .fix("wrap in parentheses", format!("({})", text));
        if starts_uppercase(text) {
            report = report.detail("references must be parenthesized");
        }
        report.emit();
        self.bump_as_error();
    }

    /// Field constraint: `field_name: pattern`
    pub(crate) fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.assert_current(SyntaxKind::Id);
        let span = self.current_span();
        let text = self.current_text();
        self.bump();
        self.validate_field_name(text, span);

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
        self.error_with_fix(DiagnosticKind::InvalidFieldEquals, span, "use `:`", ":");
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
        let is_suppressive = self.at(SyntaxKind::SuppressiveCapture);

        if !is_capture && !is_suppressive {
            return;
        }

        self.start_node_at(checkpoint, SyntaxKind::Capture);
        self.drain_trivia();

        let source = self.source;
        let span = self.current_span();
        self.bump();

        let end = self.consume_dotted_capture_tail(span.end());
        let full_span = TextRange::new(span.start(), end);
        let name = &source[usize::from(span.start()) + 1..usize::from(end)]; // strip @ prefix
        self.validate_capture_name(name, full_span);

        // Type annotation only on regular captures
        if is_capture {
            if self.at(SyntaxKind::DoubleColon) {
                self.parse_type_annotation();
            } else if self.at(SyntaxKind::Colon) {
                self.parse_type_annotation_single_colon();
            }
        }

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
