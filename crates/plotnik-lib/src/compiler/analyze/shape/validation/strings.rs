//! String-literal escape validation.
//!
//! The lexer accepts any `\<char>` pair inside a string; only the escapes
//! `unescape` understands are meaningful. This pass reports the rest, so every
//! decode site past validation can trust the escapes it sees.

use rowan::{TextRange, TextSize};

use super::ValidationInput;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk_node_pattern};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{NodePattern, TokenPattern};
use crate::compiler::parse::cst::SyntaxToken;
use crate::compiler::parse::strings::{EscapeIssueKind, unescape};

pub fn validate_strings(input: ValidationInput) {
    let ValidationInput {
        source_id,
        ast,
        diag,
    } = input;
    let mut validator = StringValidator { diag, source_id };
    validator.visit(&Located::new(source_id, ast.clone()));
}

struct StringValidator<'d> {
    diag: &'d mut Diagnostics,
    source_id: SourceId,
}

impl Visitor for StringValidator<'_> {
    fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
        if let Some(pred) = node.node().predicate()
            && let Some(value) = pred.string_value()
        {
            self.check_token(&value);
        }
        walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, node: &Located<TokenPattern>) {
        if let Some(value) = node.node().value() {
            self.check_token(&value);
        }
    }
}

impl StringValidator<'_> {
    fn check_token(&mut self, token: &SyntaxToken) {
        let (_, issues) = unescape(token.text());
        let base = token.text_range().start();
        for issue in issues {
            let start = u32::try_from(issue.range.start).expect("string token fits u32");
            let end = u32::try_from(issue.range.end).expect("string token fits u32");
            let range = TextRange::new(base + TextSize::from(start), base + TextSize::from(end));
            let kind = match issue.kind {
                EscapeIssueKind::Unknown => DiagnosticKind::UnknownStringEscape,
                EscapeIssueKind::InvalidUnicode => DiagnosticKind::InvalidUnicodeEscape,
            };
            self.diag
                .report(kind, Span::new(self.source_id, range))
                .emit();
        }
    }
}
