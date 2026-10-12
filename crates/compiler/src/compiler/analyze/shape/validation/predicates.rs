//! Predicate validation.
//!
//! Validates regex patterns in predicates for unsupported features:
//! - Backreferences (`\1`)
//! - Lookahead/lookbehind (`(?=...)`, `(?!...)`, etc.)
//! - Named captures (`(?P<name>...)`)
//! - Target-dependent line modes and word-boundary variants

use regex_syntax::ast::{self, Ast, GroupKind, Visitor as RegexVisitor, visit};
use regex_syntax::hir;
use rowan::TextRange;

use super::PredicateInput;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk_named_node_pattern};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::NamedNodePattern;

pub fn validate_predicates(input: PredicateInput) {
    let PredicateInput {
        source_id,
        ast,
        source_content,
        diag,
    } = input;
    let mut validator = PredicateValidator {
        diag,
        source_id,
        source: source_content,
    };
    validator.visit(&Located::new(source_id, ast.clone()));
}

struct PredicateValidator<'q, 'd> {
    diag: &'d mut Diagnostics,
    source_id: SourceId,
    source: &'q str,
}

#[derive(Clone, Copy)]
struct RegexLiteral<'q> {
    pattern: &'q str,
    range: TextRange,
}

impl RegexLiteral<'_> {
    fn map_span(self, regex_span: &ast::Span) -> TextRange {
        // `range` includes the `/` delimiters, so content starts at +1.
        let content_start = u32::from(self.range.start()) + 1;
        let start = content_start + regex_span.start.offset as u32;
        let end = content_start + regex_span.end.offset as u32;
        TextRange::new(start.into(), end.into())
    }
}

impl Visitor for PredicateValidator<'_, '_> {
    fn visit_named_node_pattern(&mut self, node: &Located<NamedNodePattern>) {
        let Some(predicate) = node.node().predicate() else {
            walk_named_node_pattern(self, node);
            return;
        };
        let Some(operator) = predicate.operator() else {
            walk_named_node_pattern(self, node);
            return;
        };
        if operator.is_regex_op()
            && let Some(regex) = predicate.regex()
        {
            self.validate_regex(RegexLiteral {
                pattern: regex.pattern(self.source),
                range: regex.text_range(),
            });
        }
        walk_named_node_pattern(self, node);
    }
}

impl PredicateValidator<'_, '_> {
    fn validate_regex(&mut self, regex: RegexLiteral<'_>) {
        if regex.pattern.is_empty() {
            self.diag
                .report(
                    DiagnosticKind::EmptyRegex,
                    Span::new(self.source_id, regex.range),
                )
                .emit();
            return;
        }

        // Parse with octal disabled so \1-\9 are backreferences, not octal
        let parser_result = ast::parse::ParserBuilder::new()
            .octal(false)
            .build()
            .parse(regex.pattern);

        let parsed_ast = match parser_result {
            Ok(ast) => ast,
            Err(e) => {
                let span = regex.map_span(e.span());
                let report = match e.kind() {
                    ast::ErrorKind::UnsupportedBackreference => self.diag.report(
                        DiagnosticKind::RegexBackreference,
                        Span::new(self.source_id, span),
                    ),
                    ast::ErrorKind::UnsupportedLookAround => {
                        // Skip the opening `(` - point at `?=` / `?!` / `?<=` / `?<!`
                        use rowan::TextSize;
                        let adjusted =
                            TextRange::new(span.start() + TextSize::from(1u32), span.end());
                        self.diag.report(
                            DiagnosticKind::RegexLookaround,
                            Span::new(self.source_id, adjusted),
                        )
                    }
                    _ => self
                        .diag
                        .report(
                            DiagnosticKind::RegexSyntaxError,
                            Span::new(self.source_id, span),
                        )
                        .detail(format!("{}", e.kind())),
                };
                report.emit();
                return;
            }
        };

        let detector = RegexFeatureDetector {
            named_captures: Vec::new(),
            unsupported_flags: Vec::new(),
            boundary_variants: Vec::new(),
        };
        let detector = visit(&parsed_ast, detector).expect("regex feature detection cannot fail");

        for capture_span in detector.named_captures {
            let span = regex.map_span(&capture_span);
            // The span covers `?P<name>` / `?<name>` (the `(` is excluded), so deleting it
            // turns `(?P<name>foo)` into a plain group `(foo)`.
            self.diag
                .report(
                    DiagnosticKind::RegexNamedCapture,
                    Span::new(self.source_id, span),
                )
                .fix("remove the named-capture marker", "")
                .emit();
        }

        for (flag_span, flag) in detector.unsupported_flags {
            let span = regex.map_span(&flag_span);
            let kind = match flag {
                UnsupportedFlag::Multiline => DiagnosticKind::RegexMultilineFlag,
                UnsupportedFlag::Crlf => DiagnosticKind::RegexCrlfFlag,
            };
            self.diag
                .report(kind, Span::new(self.source_id, span))
                .emit();
        }

        for boundary_span in detector.boundary_variants {
            let span = regex.map_span(&boundary_span);
            self.diag
                .report(
                    DiagnosticKind::RegexBoundaryVariant,
                    Span::new(self.source_id, span),
                )
                .emit();
        }

        self.validate_hir(regex, &parsed_ast);
    }

    fn validate_hir(&mut self, regex: RegexLiteral<'_>, parsed_ast: &Ast) {
        let mut translator = hir::translate::TranslatorBuilder::new().build();
        let Err(error) = translator.translate(regex.pattern, parsed_ast) else {
            return;
        };

        let span = regex.map_span(error.span());
        self.diag
            .report(
                DiagnosticKind::RegexSyntaxError,
                Span::new(self.source_id, span),
            )
            .detail(error.kind().to_string())
            .emit();
    }
}

struct RegexFeatureDetector {
    named_captures: Vec<ast::Span>,
    unsupported_flags: Vec<(ast::Span, UnsupportedFlag)>,
    boundary_variants: Vec<ast::Span>,
}

#[derive(Clone, Copy)]
enum UnsupportedFlag {
    Multiline,
    Crlf,
}

impl RegexVisitor for RegexFeatureDetector {
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
        if let Ast::Group(group) = ast
            && let GroupKind::NonCapturing(flags) = &group.kind
        {
            self.record_flags(flags);
        }
        if let Ast::Flags(flags) = ast {
            self.record_flags(&flags.flags);
        }
        if let Ast::Assertion(assertion) = ast
            && matches!(
                assertion.kind,
                ast::AssertionKind::WordBoundaryStart
                    | ast::AssertionKind::WordBoundaryEnd
                    | ast::AssertionKind::WordBoundaryStartAngle
                    | ast::AssertionKind::WordBoundaryEndAngle
                    | ast::AssertionKind::WordBoundaryStartHalf
                    | ast::AssertionKind::WordBoundaryEndHalf
            )
        {
            self.boundary_variants.push(assertion.span);
        }
        Ok(())
    }
}

impl RegexFeatureDetector {
    fn record_flags(&mut self, flags: &ast::Flags) {
        let mut negated = false;
        for item in &flags.items {
            match item.kind {
                ast::FlagsItemKind::Negation => negated = true,
                ast::FlagsItemKind::Flag(_) if negated => {}
                ast::FlagsItemKind::Flag(ast::Flag::MultiLine) => self
                    .unsupported_flags
                    .push((item.span, UnsupportedFlag::Multiline)),
                ast::FlagsItemKind::Flag(ast::Flag::CRLF) => self
                    .unsupported_flags
                    .push((item.span, UnsupportedFlag::Crlf)),
                ast::FlagsItemKind::Flag(_) => {}
            }
        }
    }
}
