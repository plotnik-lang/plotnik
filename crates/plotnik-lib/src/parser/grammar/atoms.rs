use crate::parser::Parser;
use crate::parser::cst::SyntaxKind;

impl Parser<'_> {
    pub(crate) fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore, "'_' wildcard");
        self.finish_node();
    }

    /// `"if"` | `'+'`
    pub(crate) fn parse_str(&mut self) {
        self.start_node(SyntaxKind::Str);
        self.bump_string_tokens();
        self.finish_node();
    }

    /// Consume string tokens (quote + optional content + quote) without creating a node.
    /// Used for contexts where string appears as a raw value (supertype, MISSING arg).
    pub(crate) fn bump_string_tokens(&mut self) {
        let open_quote = self.current();
        self.bump(); // opening quote

        if self.current() == SyntaxKind::StrVal {
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
