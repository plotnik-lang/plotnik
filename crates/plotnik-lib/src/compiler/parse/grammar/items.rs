use rowan::TextRange;

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::ROOT_PATTERN_FIRST_TOKENS;

impl Parser<'_, '_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        while !self.is_done() && !self.at(SyntaxKind::Error) {
            // LL(2): Id followed by Equals → named definition (if PascalCase)
            if self.at(SyntaxKind::Id) && self.next_is(SyntaxKind::Equals) {
                self.parse_def();
                continue;
            }

            let start = self.current_span().start();
            self.start_node(SyntaxKind::Def);
            let success = self.parse_pattern_or_error();
            if !success {
                self.error_until_next_def();
            }
            self.finish_node();
            if success {
                let end = self.last_non_trivia_end().unwrap_or(start);
                let span = TextRange::new(start, end);
                let def_text = &self.source[usize::from(start)..usize::from(end)];
                let hint = format!("give it a name like `Name = {}`", def_text.trim());
                if let Some(report) = self.report_at(DiagnosticKind::MissingDefName, span) {
                    report.hint(hint).emit();
                }
            }
        }

        self.eat_trivia();
        self.finish_node();
    }

    pub(crate) fn error_until_next_def(&mut self) {
        if self.is_done() {
            return;
        }

        if self.currently_at_def_start() {
            return;
        }

        self.start_node(SyntaxKind::Error);
        while !self.is_done() && !self.currently_at_def_start() {
            self.bump();
            self.skip_trivia_to_buffer();
        }
        self.finish_node();
    }

    pub(crate) fn currently_at_def_start(&mut self) -> bool {
        if self.at(SyntaxKind::Id) && self.next_is(SyntaxKind::Equals) {
            return true;
        }
        self.at_ts(ROOT_PATTERN_FIRST_TOKENS)
    }

    /// Named pattern definition: `Name = pattern`
    fn parse_def(&mut self) {
        self.start_node(SyntaxKind::Def);

        let ident = self.bump_ident();
        self.validate_def_name(ident);

        let ate_equals = self.eat_token(SyntaxKind::Equals);
        assert!(
            ate_equals,
            "parse_def: expected '=' but found {:?} (caller should verify Equals is present)",
            self.current()
        );

        self.parse_required_pattern();

        self.finish_node();
    }
}
