use rowan::{Checkpoint, TextRange, TextSize};

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;
use crate::parser::cst::token_sets::QUANTIFIERS;

use super::utils::starts_uppercase;

impl Parser<'_, '_> {
    /// Type annotation: `::Type` (PascalCase) or `::string` (primitive)
    pub(crate) fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon, "'::' for type annotation");

        if self.currently_is(SyntaxKind::Id) {
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

        self.bump(); // colon

        // `currently_is` skips trivia, so this handles `@x : Type` with space
        if self.currently_is(SyntaxKind::Id) {
            self.bump();
        }

        self.finish_node();
    }

    /// Negated field assertion: `-field` (field must be absent)
    ///
    /// Also accepts deprecated `!field` syntax with a warning.
    pub(crate) fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);

        // Accept both `-` (preferred) and `!` (deprecated)
        if self.currently_is(SyntaxKind::Negation) {
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

        if !self.currently_is(SyntaxKind::Id) {
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

    /// Disambiguate `field: expr` from bare identifier via LL(2) lookahead.
    /// Also handles `field = expr` typo (should be `field: expr`).
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
            report = report.message("references must be parenthesized");
        }
        report.emit();
        self.bump_as_error();
    }

    /// Field constraint: `field_name: expr`
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

        self.parse_required_expr_no_suffix();

        self.finish_node();
    }

    /// Handle `field = expr` typo - parse as Field but emit error.
    pub(crate) fn parse_field_equals_typo(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.bump();
        let span = self.current_span();
        self.error_with_fix(DiagnosticKind::InvalidFieldEquals, span, "use `:`", ":");
        self.bump();

        self.parse_required_expr_no_suffix();

        self.finish_node();
    }

    /// If current token is quantifier, wrap preceding expression using checkpoint.
    pub(crate) fn try_parse_quantifier(&mut self, checkpoint: Checkpoint) {
        if self.currently_is_one_of(QUANTIFIERS) {
            self.start_node_at(checkpoint, SyntaxKind::Quantifier);
            self.bump();
            self.finish_node();
        }
    }

    /// If current token is a capture (`@name` or `@_`), wrap preceding expression with Capture using checkpoint.
    pub(crate) fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
        let is_capture = self.currently_is(SyntaxKind::CaptureToken);
        let is_suppressive = self.currently_is(SyntaxKind::SuppressiveCapture);

        if !is_capture && !is_suppressive {
            return;
        }

        self.start_node_at(checkpoint, SyntaxKind::Capture);
        self.drain_trivia();

        let source = self.source;
        let span = self.current_span();
        self.bump(); // consume CaptureToken or SuppressiveCapture

        let end = self.consume_dotted_capture_tail(span.end());
        let full_span = TextRange::new(span.start(), end);
        let name = &source[usize::from(span.start()) + 1..usize::from(end)]; // strip @ prefix
        self.validate_capture_name(name, full_span);

        // Type annotation only on regular captures
        if is_capture {
            if self.currently_is(SyntaxKind::DoubleColon) {
                self.parse_type_annotation();
            } else if self.currently_is(SyntaxKind::Colon) {
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
