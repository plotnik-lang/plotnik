use rowan::TextRange;

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;

impl Parser<'_, '_> {
    pub(crate) fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore, "'_' wildcard");
        self.finish_node();
    }

    /// `"if"` | `'+'`
    pub(crate) fn parse_str(&mut self) {
        let start = self.current_span().start();
        self.start_node(SyntaxKind::Str);

        let open_quote = self.current();
        self.bump(); // opening quote

        let has_content = self.currently_is(SyntaxKind::StrVal);
        if has_content {
            self.bump();
        }

        let closing = self.current();
        assert_eq!(
            closing, open_quote,
            "parse_str: expected closing {:?} but found {:?} \
             (lexer should only produce quote tokens from complete strings)",
            open_quote, closing
        );
        let end = self.current_span().end();
        self.bump(); // closing quote

        self.finish_node();

        if !has_content {
            self.diagnostics
                .report(
                    self.source_id,
                    DiagnosticKind::EmptyAnonymousNode,
                    TextRange::new(start, end),
                )
                .emit();
        }
    }

    /// Consume string tokens (quote + optional content + quote) without creating a node.
    /// Used for contexts where string appears as a raw value (supertype, MISSING arg).
    pub(crate) fn bump_string_tokens(&mut self) {
        let open_quote = self.current();
        self.bump(); // opening quote

        if self.currently_is(SyntaxKind::StrVal) {
            self.bump(); // content
        }

        let closing = self.current();
        assert_eq!(
            closing, open_quote,
            "bump_string_tokens: expected closing {:?} but found {:?} \
             (lexer should only produce quote tokens from complete strings)",
            open_quote, closing
        );
        self.bump();
    }

    /// `.` anchor
    pub(crate) fn parse_anchor(&mut self) {
        self.start_node(SyntaxKind::Anchor);
        self.expect(SyntaxKind::Dot, "'.' anchor");
        self.finish_node();
    }
}
