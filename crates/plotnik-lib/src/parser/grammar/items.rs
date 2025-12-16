use rowan::TextRange;

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;
use crate::parser::cst::token_sets::{EXPR_FIRST_TOKENS, ROOT_EXPR_FIRST_TOKENS};
use crate::parser::lexer::token_text;

impl Parser<'_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        let mut unnamed_def_spans: Vec<TextRange> = Vec::new();

        while !self.should_stop() && !self.currently_is(SyntaxKind::Error) {
            if self.currently_is(SyntaxKind::Pub) {
                self.parse_def();
                continue;
            }

            // LL(2): Id followed by Equals → named definition (if PascalCase)
            if self.currently_is(SyntaxKind::Id) && self.next_is(SyntaxKind::Equals) {
                self.parse_def();
                continue;
            }

            let start = self.current_span().start();
            self.start_node(SyntaxKind::Def);
            let success = self.parse_expr_or_error();
            if !success {
                self.error_until_next_def();
            }
            self.finish_node();
            if success {
                let end = self.last_non_trivia_end().unwrap_or(start);
                unnamed_def_spans.push(TextRange::new(start, end));
            }
        }

        if unnamed_def_spans.len() > 1 {
            for span in &unnamed_def_spans[..unnamed_def_spans.len() - 1] {
                let def_text = &self.source[usize::from(span.start())..usize::from(span.end())];
                self.diagnostics
                    .report(DiagnosticKind::UnnamedDefNotLast, *span)
                    .message(format!("give it a name like `Name = {}`", def_text.trim()))
                    .emit();
            }
        }

        self.eat_trivia();
        self.finish_node();
    }

    pub(crate) fn error_until_next_def(&mut self) {
        if self.should_stop() {
            return;
        }

        // Check if already at a sync point
        if self.currently_at_def_start() {
            return;
        }

        self.start_node(SyntaxKind::Error);
        while !self.should_stop() && !self.currently_at_def_start() {
            self.bump();
            self.skip_trivia_to_buffer();
        }
        self.finish_node();
    }

    pub(crate) fn currently_at_def_start(&mut self) -> bool {
        if self.currently_is(SyntaxKind::Id) && self.next_is(SyntaxKind::Equals) {
            return true;
        }
        self.currently_is_one_of(ROOT_EXPR_FIRST_TOKENS)
    }

    /// Named expression definition: `Name = expr`
    fn parse_def(&mut self) {
        self.start_node(SyntaxKind::Def);

        self.eat_token(SyntaxKind::Pub);

        let span = self.current_span();
        let name = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_def_name(name, span);

        let ate_equals = self.eat_token(SyntaxKind::Equals);
        assert!(
            ate_equals,
            "parse_def: expected '=' but found {:?} (caller should verify Equals is present)",
            self.current()
        );

        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr();
        } else {
            self.error_msg(
                DiagnosticKind::ExpectedExpression,
                "after `=` in definition",
            );
        }

        self.finish_node();
    }
}
