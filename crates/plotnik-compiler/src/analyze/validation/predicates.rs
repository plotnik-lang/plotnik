//! Predicate validation.
//!
//! Validates regex patterns in predicates for unsupported features:
//! - Backreferences (`\1`)
//! - Lookahead/lookbehind (`(?=...)`, `(?!...)`, etc.)
//! - Named captures (`(?P<name>...)`)

use regex_syntax::ast::{self, Ast, GroupKind, Visitor as RegexVisitor, visit};
use rowan::TextRange;

use super::PredicateInput;
use crate::analyze::Reporter;
use crate::analyze::visitor::{Visitor, walk_node_pattern};
use crate::diagnostics::DiagnosticKind;
use crate::parser::NodePattern;

pub fn validate_predicates(input: PredicateInput) {
    let PredicateInput {
        source_id,
        ast,
        source_content,
        diag,
    } = input;
    let mut validator = PredicateValidator {
        reporter: Reporter::new(source_id, diag),
        source: source_content,
    };
    validator.visit(ast);
}

struct PredicateValidator<'q, 'd> {
    reporter: Reporter<'d>,
    source: &'q str,
}

impl Visitor for PredicateValidator<'_, '_> {
    fn visit_node_pattern(&mut self, node: &NodePattern) {
        if let Some(pred) = node.predicate()
            && let Some(op) = pred.operator()
            && op.is_regex_op()
            && let Some(regex) = pred.regex()
        {
            self.validate_regex(regex.pattern(self.source), regex.text_range());
        }
        walk_node_pattern(self, node);
    }
}

impl PredicateValidator<'_, '_> {
    fn validate_regex(&mut self, pattern: &str, regex_range: TextRange) {
        if pattern.is_empty() {
            self.reporter
                .report(DiagnosticKind::EmptyRegex, regex_range)
                .emit();
            return;
        }

        // Parse with octal disabled so \1-\9 are backreferences, not octal
        let parser_result = ast::parse::ParserBuilder::new()
            .octal(false)
            .build()
            .parse(pattern);

        let parsed_ast = match parser_result {
            Ok(ast) => ast,
            Err(e) => {
                let span = self.map_regex_span(e.span(), regex_range);
                let report = match e.kind() {
                    ast::ErrorKind::UnsupportedBackreference => self
                        .reporter
                        .report(DiagnosticKind::RegexBackreference, span),
                    ast::ErrorKind::UnsupportedLookAround => {
                        // Skip the opening `(` - point at `?=` / `?!` / `?<=` / `?<!`
                        use rowan::TextSize;
                        let adjusted =
                            TextRange::new(span.start() + TextSize::from(1u32), span.end());
                        self.reporter
                            .report(DiagnosticKind::RegexLookaround, adjusted)
                    }
                    _ => self
                        .reporter
                        .report(DiagnosticKind::RegexSyntaxError, span)
                        .detail(format!("{}", e.kind())),
                };
                report.emit();
                return;
            }
        };

        let detector = NamedCaptureDetector {
            named_captures: Vec::new(),
        };
        let detector = visit(&parsed_ast, detector).unwrap();

        for capture_span in detector.named_captures {
            let span = self.map_regex_span(&capture_span, regex_range);
            // The span covers `?P<name>` / `?<name>` (the `(` is excluded), so deleting it
            // turns `(?P<name>foo)` into a plain group `(foo)`.
            self.reporter
                .report(DiagnosticKind::RegexNamedCapture, span)
                .fix("remove the named-capture marker", "")
                .emit();
        }
    }

    fn map_regex_span(&self, regex_span: &ast::Span, regex_range: TextRange) -> TextRange {
        // regex_range includes the `/` delimiters, so content starts at +1
        let content_start = u32::from(regex_range.start()) + 1;
        let start = content_start + regex_span.start.offset as u32;
        let end = content_start + regex_span.end.offset as u32;
        TextRange::new(start.into(), end.into())
    }
}

struct NamedCaptureDetector {
    named_captures: Vec<ast::Span>,
}

impl RegexVisitor for NamedCaptureDetector {
    type Output = Self;
    type Err = std::convert::Infallible;

    fn finish(self) -> Result<Self::Output, Self::Err> {
        Ok(self)
    }

    fn visit_pre(&mut self, ast: &Ast) -> Result<(), Self::Err> {
        if let Ast::Group(group) = ast
            && let GroupKind::CaptureName { name, .. } = &group.kind
        {
            // Span for `?P<name>` (skip opening paren, include closing `>`)
            let start = ast::Position::new(
                group.span.start.offset + 1,
                group.span.start.line,
                group.span.start.column + 1,
            );
            let end = ast::Position::new(
                name.span.end.offset + 1,
                name.span.end.line,
                name.span.end.column + 1,
            );
            self.named_captures.push(ast::Span::new(start, end));
        }
        Ok(())
    }
}
