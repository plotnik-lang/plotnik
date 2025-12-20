use rowan::Checkpoint;

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;
use crate::parser::cst::token_sets::{EXPR_FIRST_TOKENS, QUANTIFIERS};
use crate::parser::lexer::token_text;

impl Parser<'_, '_> {
    /// `@name` | `@name :: Type`
    pub(crate) fn parse_capture_suffix(&mut self) {
        self.bump(); // consume At

        if !self.currently_is(SyntaxKind::Id) {
            self.error(DiagnosticKind::ExpectedCaptureName);
            return;
        }

        let span = self.current_span();
        let name = token_text(self.source, &self.tokens[self.pos]);
        self.bump(); // consume Id

        self.validate_capture_name(name, span);

        if self.currently_is(SyntaxKind::Colon) {
            self.parse_type_annotation_single_colon();
            return;
        }
        if self.currently_is(SyntaxKind::DoubleColon) {
            self.parse_type_annotation();
        }
    }

    /// Type annotation: `::Type` (PascalCase) or `::string` (primitive)
    pub(crate) fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon, "'::' for type annotation");

        if self.currently_is(SyntaxKind::Id) {
            let span = self.current_span();
            let text = token_text(self.source, &self.tokens[self.pos]);
            self.bump();
            self.validate_type_name(text, span);
        } else {
            self.error_msg(
                DiagnosticKind::ExpectedTypeName,
                "e.g., `::MyType` or `::string`",
            );
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
            "single `:` looks like a field",
            "use `::`",
            "::",
        );

        self.bump(); // colon

        // peek() skips whitespace, so this handles `@x : Type` with space
        if self.currently_is(SyntaxKind::Id) {
            self.bump();
        }

        self.finish_node();
    }

    /// Negated field assertion: `!field` (field must be absent)
    pub(crate) fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation, "'!' for negated field");

        if !self.currently_is(SyntaxKind::Id) {
            self.error_msg(DiagnosticKind::ExpectedFieldName, "e.g., `!value`");
            self.finish_node();
            return;
        }

        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
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

        // Bare identifiers are not valid expressions; trees require parentheses
        self.error_and_bump_msg(
            DiagnosticKind::BareIdentifier,
            "wrap in parentheses: `(identifier)`",
        );
    }

    /// Field constraint: `field_name: expr`
    pub(crate) fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.assert_current(SyntaxKind::Id);
        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_field_name(text, span);

        self.expect(
            SyntaxKind::Colon,
            "':' to separate field name from its value",
        );

        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr_no_suffix();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `field:`");
        }

        self.finish_node();
    }

    /// Handle `field = expr` typo - parse as Field but emit error.
    pub(crate) fn parse_field_equals_typo(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.bump();
        let span = self.current_span();
        self.error_with_fix(
            DiagnosticKind::InvalidFieldEquals,
            span,
            "this isn't a definition",
            "use `:`",
            ":",
        );
        self.bump();

        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `field =`");
        }

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

    /// If current token is a capture (`@name`), wrap preceding expression with Capture using checkpoint.
    pub(crate) fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
        if self.currently_is(SyntaxKind::At) {
            self.start_node_at(checkpoint, SyntaxKind::Capture);
            self.drain_trivia();
            self.parse_capture_suffix();
            self.finish_node();
        }
    }
}
