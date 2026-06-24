use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::{EXPR_FIRST_TOKENS, QUANTIFIERS};

/// Whether a parsed expression should absorb a trailing quantifier/capture suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuffixMode {
    Apply,
    Skip,
}

impl Parser<'_, '_> {
    /// Parse an expression, or emit an error if current token can't start one.
    /// Returns `true` if a valid expression was parsed, `false` on error.
    pub(crate) fn parse_pattern_or_error(&mut self) -> bool {
        if self.at_ts(EXPR_FIRST_TOKENS) {
            self.parse_pattern();
            return true;
        }

        if self.at(SyntaxKind::At) {
            self.error_and_bump(DiagnosticKind::CaptureWithoutTarget);
            return false;
        }

        if self.at_ts_predicate() {
            self.error_unsupported_predicate();
            return false;
        }

        self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
            report.hint("try `(node)`, `[a b]`, `{a b}`, `\"literal\"`, or `_`")
        });
        false
    }

    /// Parse an expression required after a prefix like `=`, `field:`, or a branch label.
    ///
    /// On a non-expression token this reports `ExpectedExpression` at the current position,
    /// except a misplaced tree-sitter predicate (`#eq?`), which gets its dedicated diagnostic
    /// instead of the generic one — these are exactly the spots a tree-sitter user pastes them.
    pub(crate) fn parse_required_pattern(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Apply)
    }

    /// Like [`Self::parse_required_pattern`], but without applying a quantifier/capture suffix —
    /// for field values, so the suffix wraps the whole field constraint (see
    /// [`Self::parse_pattern_no_suffix`]).
    pub(crate) fn parse_required_pattern_no_suffix(&mut self) {
        self.parse_required_pattern_inner(SuffixMode::Skip)
    }

    fn parse_required_pattern_inner(&mut self, suffix: SuffixMode) {
        if self.at_ts(EXPR_FIRST_TOKENS) {
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

    pub(crate) fn parse_pattern(&mut self) {
        self.parse_pattern_inner(SuffixMode::Apply)
    }

    /// Parse expression without applying quantifier/capture suffix.
    ///
    /// Used for field values so that suffixes apply to the whole field constraint:
    /// - `field: (x)*` parses as `(field: (x))*` — repeat the field (e.g., decorators)
    /// - `field: (x) @cap` parses as `(field: (x)) @cap` — capture the field expression
    ///
    /// For captures on structured values (enums/structs), the compilation handles this
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
        } else if suffix == SuffixMode::Apply {
            self.try_parse_quantifier(checkpoint);
            self.try_parse_capture(checkpoint);
        }

        self.exit_recursion();
    }

    fn reject_anchor_suffixes(&mut self) {
        loop {
            if self.at_ts(QUANTIFIERS) {
                self.report_current_and_bump(DiagnosticKind::QuantifiedAnchor, |report| {
                    report.fix("remove the quantifier", "")
                });
            } else if matches!(
                self.current(),
                SyntaxKind::CaptureToken | SyntaxKind::SuppressiveCapture
            ) {
                self.report_current_and_bump(DiagnosticKind::CapturedAnchor, |report| {
                    report.fix("remove the capture", "")
                });
            } else {
                return;
            }
        }
    }
}
