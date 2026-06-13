use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;
use crate::parser::cst::token_sets::{EXPR_FIRST_TOKENS, QUANTIFIERS};

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

        if self.at_ts_predicate() {
            self.error_unsupported_predicate();
            return false;
        }

        self.error_and_bump_with_hint(
            DiagnosticKind::UnexpectedToken,
            "try `(node)`, `[a b]`, `{a b}`, `\"literal\"`, or `_`",
        );
        false
    }

    /// Parse an expression required after a prefix like `=`, `field:`, or a branch label.
    ///
    /// On a non-expression token this reports `ExpectedExpression` at the current position,
    /// except a misplaced tree-sitter predicate (`#eq?`), which gets its dedicated diagnostic
    /// instead of the generic one — these are exactly the spots a tree-sitter user pastes them.
    pub(crate) fn parse_required_expr(&mut self) {
        self.parse_required_expr_inner(true)
    }

    /// Like [`Self::parse_required_expr`], but without applying a quantifier/capture suffix —
    /// for field values, so the suffix wraps the whole field constraint (see
    /// [`Self::parse_expr_no_suffix`]).
    pub(crate) fn parse_required_expr_no_suffix(&mut self) {
        self.parse_required_expr_inner(false)
    }

    fn parse_required_expr_inner(&mut self, with_suffix: bool) {
        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            if with_suffix {
                self.parse_expr();
            } else {
                self.parse_expr_no_suffix();
            }
            return;
        }

        if self.at_ts_predicate() {
            self.error_unsupported_predicate();
            return;
        }

        self.error(DiagnosticKind::ExpectedExpression);
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

        let kind = self.current();
        match kind {
            SyntaxKind::ParenOpen => self.parse_tree(),
            SyntaxKind::BracketOpen => self.parse_alt(),
            SyntaxKind::BraceOpen => self.parse_seq(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.parse_str(),
            SyntaxKind::UnterminatedString => {
                self.error_and_bump(DiagnosticKind::UnclosedString);
            }
            SyntaxKind::Dot | SyntaxKind::DotBang => self.parse_anchor(),
            SyntaxKind::Negation | SyntaxKind::Minus => self.parse_negated_field(),
            SyntaxKind::Id => self.parse_tree_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(DiagnosticKind::ErrorMissingOutsideParens);
            }
            _ => {
                self.error_and_bump(DiagnosticKind::UnexpectedToken);
            }
        }

        if matches!(kind, SyntaxKind::Dot | SyntaxKind::DotBang) {
            // Anchors constrain position and produce no value: `*` or `@x` after
            // one is always a mistake, never a suffix to wrap.
            self.reject_anchor_suffixes();
        } else if with_suffix {
            self.try_parse_quantifier(checkpoint);
            self.try_parse_capture(checkpoint);
        }

        self.exit_recursion();
    }

    fn reject_anchor_suffixes(&mut self) {
        loop {
            if self.currently_is_one_of(QUANTIFIERS) {
                self.error_and_bump(DiagnosticKind::QuantifiedAnchor);
            } else if matches!(
                self.current(),
                SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture
            ) {
                self.error_and_bump(DiagnosticKind::CapturedAnchor);
            } else {
                return;
            }
        }
    }
}
