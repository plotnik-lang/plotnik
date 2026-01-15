use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;
use crate::parser::cst::token_sets::EXPR_FIRST_TOKENS;

impl Parser<'_, '_> {
    /// Parse an expression, or emit an error if current token can't start one.
    /// Returns `true` if a valid expression was parsed, `false` on error.
    pub(crate) fn parse_expr_or_error(&mut self) -> bool {
        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr();
            return true;
        }

        if self.currently_is(SyntaxKind::At) {
            self.error_and_bump(DiagnosticKind::CaptureWithoutTarget);
            return false;
        }

        if self.currently_is(SyntaxKind::TsPredicate) {
            self.error_and_bump(DiagnosticKind::UnsupportedPredicate);
            return false;
        }

        self.error_and_bump_with_hint(
            DiagnosticKind::UnexpectedToken,
            "try `(node)`, `[a b]`, `{a b}`, `\"literal\"`, or `_`",
        );
        false
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier/capture suffix.
    pub(crate) fn parse_expr(&mut self) {
        self.parse_expr_inner(true)
    }

    /// Parse expression without applying quantifier/capture suffix.
    ///
    /// Used for field values so that suffixes apply to the whole field constraint:
    /// - `field: (x)*` parses as `(field: (x))*` — repeat the field (e.g., decorators)
    /// - `field: (x) @cap` parses as `(field: (x)) @cap` — capture the field expression
    ///
    /// For captures on structured values (enums/structs), the compilation handles this
    /// by looking through FieldExpr to determine the actual value type. See
    /// `build_capture_effects` in compile/capture.rs.
    pub(crate) fn parse_expr_no_suffix(&mut self) {
        self.parse_expr_inner(false)
    }

    fn parse_expr_inner(&mut self, with_suffix: bool) {
        if !self.enter_recursion() {
            self.start_node(SyntaxKind::Error);
            while !self.should_stop() {
                self.bump();
            }
            self.finish_node();
            return;
        }

        let checkpoint = self.checkpoint();

        match self.current() {
            SyntaxKind::ParenOpen => self.parse_tree(),
            SyntaxKind::BracketOpen => self.parse_alt(),
            SyntaxKind::BraceOpen => self.parse_seq(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.parse_str(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation | SyntaxKind::Minus => self.parse_negated_field(),
            SyntaxKind::Id => self.parse_tree_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(DiagnosticKind::ErrorMissingOutsideParens);
            }
            _ => {
                self.error_and_bump(DiagnosticKind::UnexpectedToken);
            }
        }

        if with_suffix {
            self.try_parse_quantifier(checkpoint);
            self.try_parse_capture(checkpoint);
        }

        self.exit_recursion();
    }
}
