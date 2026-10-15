use rowan::{Checkpoint, TextRange};

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::{
    ALT_RECOVERY_TOKENS, NODE_RECOVERY_TOKENS, PATTERN_FIRST_TOKENS, PREDICATE_OPS, SEPARATORS,
    SEQ_RECOVERY_TOKENS, TokenSet,
};

use super::utils::{starts_uppercase, to_pascal_case};

enum ParenHead<'q> {
    Concrete,
    DefRef(&'q str),
}

#[derive(Default)]
struct TreeSitterPredicateArguments<'q> {
    target: Option<&'q str>,
    value: Option<&'q str>,
}

fn predicate_operator(name: &str) -> Option<&'static str> {
    match name {
        "eq" => Some("=="),
        "not-eq" => Some("!="),
        "match" => Some("=~"),
        "not-match" => Some("!~"),
        _ => None,
    }
}

impl<'q> Parser<'q, '_> {
    pub(crate) fn parse_named_node(&mut self) {
        let checkpoint = self.checkpoint();
        self.push_delimiter();
        let open_paren_span = self.current_span();
        let mut tree_sitter_sequence = false;
        self.bump();

        let head = match self.current() {
            SyntaxKind::ParenClose => {
                self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                ParenHead::Concrete
            }
            SyntaxKind::Underscore => {
                self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                self.bump();
                ParenHead::Concrete
            }
            SyntaxKind::Id => self.parse_id_ref_or_node(checkpoint),
            SyntaxKind::KwError => {
                self.parse_error_node(checkpoint);
                return;
            }
            SyntaxKind::KwMissing => {
                self.parse_missing_node(checkpoint);
                return;
            }
            _ => {
                if self.at_ts_predicate() {
                    self.parse_node_predicate_error(checkpoint);
                    return;
                }
                if self.at_ts(PATTERN_FIRST_TOKENS) {
                    self.start_node_at(checkpoint, SyntaxKind::Sequence);
                    tree_sitter_sequence = true;
                } else {
                    self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                }
                ParenHead::Concrete
            }
        };

        self.finish_named_node_parsing(checkpoint, head);
        if tree_sitter_sequence {
            self.report_tree_sitter_sequence(open_paren_span);
        }
    }

    fn parse_id_ref_or_node(&mut self, checkpoint: Checkpoint) -> ParenHead<'q> {
        let name = self.current_text();
        let head_end = self.current_span().end();
        self.bump();

        let head = if starts_uppercase(name) {
            ParenHead::DefRef(name)
        } else {
            self.start_node_at(checkpoint, SyntaxKind::NamedNode);
            ParenHead::Concrete
        };

        let head = if let Some(sep) = self.tokens.get(self.pos).copied()
            && matches!(sep.kind, SyntaxKind::Slash | SyntaxKind::Hash)
            && sep.span.start() == head_end
        {
            match head {
                ParenHead::Concrete => {
                    self.parse_concrete_category_refinement(sep.kind);
                    ParenHead::Concrete
                }
                ParenHead::DefRef(_) => {
                    self.reject_ref_category_refinement(checkpoint, sep.kind);
                    ParenHead::Concrete
                }
            }
        } else {
            head
        };

        if matches!(head, ParenHead::Concrete) && self.at_ts(PREDICATE_OPS) {
            self.parse_node_predicate(name);
        }

        head
    }

    fn parse_concrete_category_refinement(&mut self, sep: SyntaxKind) {
        let sep_span = self.current_span();
        let has_subtype = self.consume_category_refinement_subtype(sep_span);

        if sep != SyntaxKind::Slash {
            return;
        }

        if has_subtype {
            if let Some(report) = self.report_at(DiagnosticKind::SupertypeSlashDeprecated, sep_span)
            {
                report.fix("use `#`", "#").emit();
            }
            return;
        }

        if let Some(report) = self.report_current(DiagnosticKind::ExpectedSubtype) {
            report
                .detail("`/` must be followed immediately by a subtype")
                .emit();
        }
    }

    fn reject_ref_category_refinement(&mut self, checkpoint: Checkpoint, sep: SyntaxKind) {
        self.start_node_at(checkpoint, SyntaxKind::NamedNode);
        if let Some(report) = self.report_current(DiagnosticKind::InvalidSupertypeSyntax) {
            report.emit();
        }

        let sep_span = self.current_span();
        let has_subtype = self.consume_category_refinement_subtype(sep_span);

        if sep == SyntaxKind::Slash
            && !has_subtype
            && let Some(report) = self.report_current(DiagnosticKind::ExpectedSubtype)
        {
            report.emit();
        }
    }

    fn consume_category_refinement_subtype(&mut self, sep_span: TextRange) -> bool {
        self.bump();
        let subtype = self.tokens.get(self.pos).copied();
        let tight = subtype.is_some_and(|t| t.span.start() == sep_span.end());
        match subtype.map(|t| t.kind) {
            Some(SyntaxKind::Id) if tight => {
                self.bump();
                true
            }
            Some(SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote) if tight => {
                self.skip_string_tokens();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn at_ts_predicate(&mut self) -> bool {
        if self.current() != SyntaxKind::Hash {
            return false;
        }
        let hash_end = self.current_span().end();
        let Some(id) = self.tokens.get(self.pos + 1).copied() else {
            return false;
        };
        if id.kind != SyntaxKind::Id || id.span.start() != hash_end {
            return false;
        }
        self.tokens.get(self.pos + 2).is_some_and(|t| {
            matches!(t.kind, SyntaxKind::Question | SyntaxKind::Negation)
                && t.span.start() == id.span.end()
        })
    }

    pub(crate) fn error_unsupported_predicate(&mut self) {
        self.start_node(SyntaxKind::Error);
        let (span, name) = self.consume_predicate_name();
        let arguments = self.inspect_tree_sitter_predicate_arguments();
        self.consume_tree_sitter_predicate_arguments(&arguments);
        self.finish_node();

        self.report_unsupported_predicate(span, name, arguments);
    }

    fn consume_predicate_name(&mut self) -> (TextRange, &'q str) {
        let start = self.current_span().start();
        self.bump();
        let name = self.current_text();
        let mut end = self.current_span().end();
        self.bump();
        if let Some(suffix) = self.tokens.get(self.pos).copied()
            && matches!(suffix.kind, SyntaxKind::Question | SyntaxKind::Negation)
            && suffix.span.start() == end
        {
            self.bump();
            end = suffix.span.end();
        }
        (TextRange::new(start, end), name)
    }

    fn parse_node_predicate_error(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Error);
        let (span, name) = self.consume_predicate_name();
        let arguments = self.inspect_tree_sitter_predicate_arguments();
        self.consume_until_matching_paren();
        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
        self.report_unsupported_predicate(span, name, arguments);
    }

    fn inspect_tree_sitter_predicate_arguments(&mut self) -> TreeSitterPredicateArguments<'q> {
        self.skip_trivia_to_buffer();
        let mut pos = self.pos;
        let mut arguments = TreeSitterPredicateArguments::default();

        if let Some(token) = self.tokens.get(pos)
            && token.kind == SyntaxKind::CaptureToken
        {
            arguments.target = Some(self.source_text(token.span));
            pos += 1;
        }

        while self
            .tokens
            .get(pos)
            .is_some_and(|token| token.kind.is_trivia())
        {
            pos += 1;
        }
        let Some(value) = self.tokens.get(pos) else {
            return arguments;
        };
        match value.kind {
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                let quote = value.kind;
                let start = value.span.start();
                pos += 1;
                while let Some(token) = self.tokens.get(pos) {
                    if token.kind == quote {
                        arguments.value =
                            Some(self.source_text(TextRange::new(start, token.span.end())));
                        break;
                    }
                    pos += 1;
                }
            }
            SyntaxKind::RegexLiteral => {
                arguments.value = Some(self.source_text(value.span));
            }
            _ => {}
        }
        arguments
    }

    fn consume_tree_sitter_predicate_arguments(
        &mut self,
        arguments: &TreeSitterPredicateArguments<'_>,
    ) {
        if arguments.target.is_some() {
            self.bump();
        }
        if arguments.value.is_none() {
            return;
        }
        match self.current() {
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.skip_string_tokens(),
            SyntaxKind::RegexLiteral => self.bump(),
            _ => unreachable!("inspected predicate value remains current"),
        }
    }

    fn consume_until_matching_paren(&mut self) {
        let mut depth = 0u32;
        while !self.eof() && !self.has_fatal_error() {
            match self.current() {
                SyntaxKind::ParenOpen | SyntaxKind::BracketOpen | SyntaxKind::BraceOpen => {
                    depth += 1;
                }
                SyntaxKind::ParenClose | SyntaxKind::BracketClose | SyntaxKind::BraceClose => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn report_unsupported_predicate(
        &mut self,
        span: TextRange,
        name: &str,
        arguments: TreeSitterPredicateArguments<'_>,
    ) {
        let hint = match (predicate_operator(name), arguments.target, arguments.value) {
            (Some(operator @ ("==" | "!=")), Some(target), Some(value)) => Some(format!(
                "move `{operator} {value}` into the node pattern captured as `{target}`"
            )),
            (Some(operator @ ("=~" | "!~")), Some(target), Some(value)) => Some(format!(
                "move a `{operator}` predicate for {value} into the node pattern captured as `{target}`"
            )),
            (Some(operator), Some(target), None) => Some(format!(
                "`#{name}?` corresponds to `{operator}` inside the node pattern captured as `{target}`, but this call has no value"
            )),
            (Some(operator), _, _) => Some(format!(
                "`#{name}?` corresponds to `{operator}` inside a node pattern, but this call does not provide both a captured target and a compatible value"
            )),
            (None, _, _) => None,
        };
        let Some(report) = self.report_at(DiagnosticKind::UnsupportedPredicate, span) else {
            return;
        };

        match hint {
            Some(hint) => report.hint(hint).emit(),
            None => report.emit(),
        }
    }

    fn parse_node_predicate(&mut self, node_name: &str) {
        self.start_node(SyntaxKind::NodePredicate);

        self.bump();

        match self.current() {
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                self.skip_string_tokens();
            }
            SyntaxKind::UnterminatedString => {
                if let Some(report) = self.report_current(DiagnosticKind::UnclosedString) {
                    report
                        .hint(format!(
                            "close the quoted value in the predicate on `{node_name}`"
                        ))
                        .emit();
                }
                self.bump_as_error();
            }
            SyntaxKind::RegexLiteral => {
                self.start_node(SyntaxKind::Regex);
                if regex_literal_is_closed(self.current_text()) {
                    self.bump();
                } else {
                    self.error_and_bump(DiagnosticKind::UnclosedRegex);
                }
                self.finish_node();
            }
            SyntaxKind::Slash => {
                self.parse_regex_literal();
            }
            _ => {
                if let Some(report) = self.report_current(DiagnosticKind::ExpectedPredicateValue) {
                    report.emit();
                }
            }
        }

        self.finish_node();
    }

    fn report_tree_sitter_sequence(&mut self, open_paren_span: TextRange) {
        let Some(end) = self.last_non_trivia_end() else {
            return;
        };
        let range = TextRange::new(open_paren_span.start(), end);
        let written = self.source_text(range).to_string();
        let Some(inner) = written
            .strip_prefix('(')
            .and_then(|text| text.strip_suffix(')'))
        else {
            return;
        };
        let replacement = format!("{{{inner}}}");
        if let Some(report) =
            self.report_at(DiagnosticKind::TreeSitterSequenceSyntaxDeprecated, range)
        {
            let detail = if written.contains(['\r', '\n']) {
                "this parenthesized group uses Tree-sitter syntax for a sibling sequence"
                    .to_string()
            } else {
                format!("`{written}` uses Tree-sitter parentheses for a sibling sequence")
            };
            report
                .detail(detail)
                .fix("replace the outer parentheses with braces", replacement)
                .emit();
        }
    }

    fn source_text(&self, range: TextRange) -> &'q str {
        &self.source[usize::from(range.start())..usize::from(range.end())]
    }

    fn parse_regex_literal(&mut self) {
        self.start_node(SyntaxKind::Regex);
        self.bump();

        let mut found_close = false;

        while !self.eof() && !self.has_fatal_error() {
            let kind = self.nth_raw(0);

            if kind != SyntaxKind::Slash {
                self.bump();
                continue;
            }

            let slash_start: usize = self.tokens[self.pos].span.start().into();
            let backslash_count = self.source[..slash_start]
                .chars()
                .rev()
                .take_while(|&c| c == '\\')
                .count();

            if backslash_count % 2 == 1 {
                self.bump();
                continue;
            }

            found_close = true;
            break;
        }

        if found_close {
            self.bump();
        } else {
            if let Some(report) = self.report_current(DiagnosticKind::UnclosedRegex) {
                report.emit();
            }
        }

        self.finish_node();
    }

    fn parse_error_node(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::NamedNode);
        self.bump();
        if !self.at(SyntaxKind::ParenClose) && !self.eof() {
            self.parse_forbidden_children(DiagnosticKind::ErrorTakesNoArguments);
        }
        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
    }

    fn parse_missing_node(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::NamedNode);
        self.bump();
        match self.current() {
            SyntaxKind::Id => {
                self.bump();
            }
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                self.skip_string_tokens();
            }
            _ => {}
        }
        if !self.at(SyntaxKind::ParenClose) && !self.eof() {
            self.parse_forbidden_children(DiagnosticKind::MissingTakesNoChildren);
        }
        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
    }

    fn parse_forbidden_children(&mut self, kind: DiagnosticKind) {
        let children_start = self.current_span().start();
        self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
        let children_end = self
            .last_non_trivia_end()
            .map_or(children_start, |end| end.max(children_start));
        let children_span = TextRange::new(children_start, children_end);
        self.diagnostics
            .report(kind, Span::new(self.source_id, children_span))
            .fix("remove the children", "")
            .emit();
    }

    fn finish_named_node_parsing(&mut self, checkpoint: Checkpoint, head: ParenHead<'q>) {
        let has_children = !self.at(SyntaxKind::ParenClose) && !self.eof();

        match head {
            ParenHead::DefRef(name) if has_children => {
                self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                let children_start = self.current_span().start();
                self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
                let children_end = self
                    .last_non_trivia_end()
                    .map_or(children_start, |end| end.max(children_start));
                let children_span = TextRange::new(children_start, children_end);

                if let Some(report) =
                    self.report_at(DiagnosticKind::RefCannotHaveChildren, children_span)
                {
                    report.detail(name).fix("remove the children", "").emit();
                }
            }
            ParenHead::DefRef(_) => {
                self.start_node_at(checkpoint, SyntaxKind::DefRef);
            }
            ParenHead::Concrete => {
                self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
            }
        }

        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
    }

    fn parse_children(&mut self, until: SyntaxKind, recovery: TokenSet) {
        loop {
            if self.eof() {
                let (construct, kind) = match until {
                    SyntaxKind::ParenClose => ("node", DiagnosticKind::UnclosedTree),
                    SyntaxKind::BraceClose => ("sequence", DiagnosticKind::UnclosedSequence),
                    _ => panic!(
                        "parse_children: unexpected delimiter {:?} (only ParenClose/BraceClose supported)",
                        until
                    ),
                };
                self.error_unclosed_at_eof(kind, construct);
                break;
            }
            if self.has_fatal_error() {
                break;
            }
            if self.at(until) {
                break;
            }
            if self.consume_stray_closer_if_expected_later(until) {
                continue;
            }
            if self.at_ts(SEPARATORS) {
                self.error_skip_separator();
                continue;
            }
            if until == SyntaxKind::BraceClose
                && matches!(self.current(), SyntaxKind::Minus | SyntaxKind::Negation)
            {
                self.parse_rejected_positional(DiagnosticKind::NegatedFieldInSequence);
                continue;
            }
            if self.at_ts(PATTERN_FIRST_TOKENS) {
                self.parse_pattern();
                continue;
            }
            if self.at_ts_predicate() {
                self.error_unsupported_predicate();
                continue;
            }
            if self.at_ts(recovery) {
                break;
            }
            self.report_unexpected_children_run(until, recovery);
        }
    }

    fn report_unexpected_children_run(&mut self, until: SyntaxKind, recovery: TokenSet) {
        let detail = match until {
            SyntaxKind::BraceClose => "expected an item, or `}` to close",
            _ => "expected a child node, or `)` to close",
        };
        let Some(range) = self.consume_unexpected_run(|parser| {
            parser.at(until)
                || parser.at_ts(SEPARATORS)
                || parser.at_ts(PATTERN_FIRST_TOKENS)
                || parser.at_ts(recovery)
                || parser.at_ts_predicate()
        }) else {
            return;
        };
        if let Some(report) = self.report_at(DiagnosticKind::UnexpectedToken, range) {
            report.detail(detail).emit();
        }
    }

    pub(crate) fn parse_alternation(&mut self) {
        self.start_node(SyntaxKind::Alternation);
        self.push_delimiter();
        self.assert_current(SyntaxKind::BracketOpen);
        self.bump();

        self.parse_alternation_children();

        self.pop_delimiter();
        self.expect_close(
            SyntaxKind::BracketClose,
            DiagnosticKind::UnclosedAlternation,
        );
        self.finish_node();
    }

    fn parse_alternation_children(&mut self) {
        loop {
            if self.eof() {
                self.error_unclosed_at_eof(DiagnosticKind::UnclosedAlternation, "alternation");
                break;
            }
            if self.has_fatal_error() {
                break;
            }
            if self.at(SyntaxKind::BracketClose) {
                break;
            }
            if self.consume_stray_closer_if_expected_later(SyntaxKind::BracketClose) {
                continue;
            }
            if self.at_ts(SEPARATORS) {
                self.error_skip_separator();
                continue;
            }

            if self.at(SyntaxKind::Id) && self.next_is(SyntaxKind::Colon) {
                if starts_uppercase(self.current_text()) {
                    self.parse_alternative();
                } else {
                    self.parse_alternative_lowercase_label();
                }
                continue;
            }
            if matches!(self.current(), SyntaxKind::Dot | SyntaxKind::DotBang) {
                self.parse_rejected_positional(DiagnosticKind::AnchorInAlternation);
                continue;
            }
            if matches!(self.current(), SyntaxKind::Minus | SyntaxKind::Negation) {
                self.parse_rejected_positional(DiagnosticKind::NegatedFieldInAlternation);
                continue;
            }
            if self.at_ts_predicate() {
                self.error_unsupported_predicate();
                continue;
            }
            if self.at_ts(PATTERN_FIRST_TOKENS) {
                self.start_node(SyntaxKind::Alternative);
                self.parse_pattern();
                self.finish_node();
                continue;
            }
            if self.at_ts(ALT_RECOVERY_TOKENS) {
                break;
            }
            self.report_unexpected_alternative_run();
        }
    }

    fn report_unexpected_alternative_run(&mut self) {
        let Some(range) = self.consume_unexpected_run(|parser| {
            parser.at(SyntaxKind::BracketClose)
                || parser.at_ts(SEPARATORS)
                || (parser.at(SyntaxKind::Id) && parser.next_is(SyntaxKind::Colon))
                || matches!(parser.current(), SyntaxKind::Dot | SyntaxKind::DotBang)
                || parser.at_ts_predicate()
                || parser.at_ts(PATTERN_FIRST_TOKENS)
                || parser.at_ts(ALT_RECOVERY_TOKENS)
        }) else {
            return;
        };
        if let Some(report) = self.report_at(DiagnosticKind::UnexpectedToken, range) {
            report
                .detail("expected an alternative, or `]` to close")
                .emit();
        }
    }

    fn consume_unexpected_run(
        &mut self,
        mut boundary: impl FnMut(&mut Self) -> bool,
    ) -> Option<TextRange> {
        let start = self.current_span().start();
        if self.eof() {
            return None;
        }
        self.start_node(SyntaxKind::Error);
        self.bump();
        loop {
            let stop = self.has_fatal_error() || boundary(self) || self.eof();
            if stop {
                break;
            }
            self.bump();
        }
        let end = self.last_non_trivia_end().unwrap_or(start);
        self.finish_node();
        Some(TextRange::new(start, end))
    }

    fn parse_alternative(&mut self) {
        self.start_node(SyntaxKind::Alternative);

        let ident = self.bump_ident();
        self.validate_alternative_label(ident);

        self.expect(SyntaxKind::Colon, "':' after alternative label");

        self.parse_alternative_body();

        self.finish_node();
    }

    fn parse_alternative_lowercase_label(&mut self) {
        self.start_node(SyntaxKind::Alternative);

        let span = self.current_span();
        let label_text = self.current_text();
        let pascal = to_pascal_case(label_text);

        if let Some(report) = self.report_at(DiagnosticKind::AlternativeLabelInvalid, span) {
            report.fix(format!("use `{}`", pascal), pascal).emit();
        }

        self.bump();
        self.expect(SyntaxKind::Colon, "':' after alternative label");

        self.parse_alternative_body();

        self.finish_node();
    }

    pub(crate) fn parse_sequence(&mut self) {
        self.start_node(SyntaxKind::Sequence);
        self.push_delimiter();
        self.assert_current(SyntaxKind::BraceOpen);
        self.bump();

        self.parse_children(SyntaxKind::BraceClose, SEQ_RECOVERY_TOKENS);

        self.pop_delimiter();
        self.expect_close(SyntaxKind::BraceClose, DiagnosticKind::UnclosedSequence);
        self.finish_node();
    }

    fn consume_stray_closer_if_expected_later(&mut self, expected: SyntaxKind) -> bool {
        let actual = self.current();
        if !matches!(
            actual,
            SyntaxKind::ParenClose | SyntaxKind::BracketClose | SyntaxKind::BraceClose
        ) || actual == expected
            || !self.tokens[self.pos + 1..]
                .iter()
                .any(|token| token.kind == expected)
        {
            return false;
        }

        let (construct, expected_text) = match expected {
            SyntaxKind::ParenClose => ("node", ")"),
            SyntaxKind::BracketClose => ("alternation", "]"),
            SyntaxKind::BraceClose => ("sequence", "}"),
            _ => unreachable!("only closing delimiters reach delimiter recovery"),
        };
        let actual_text = self.current_text().to_string();
        self.report_current_and_bump(DiagnosticKind::UnexpectedToken, |report| {
            report
                .detail(format!(
                    "unexpected `{actual_text}` inside this {construct}: the matching closer is `{expected_text}`"
                ))
                .fix(format!("remove the stray `{actual_text}`"), "")
        });
        true
    }

    fn error_skip_separator(&mut self) {
        let kind = self.current();
        let span = self.current_span();
        let char_name = match kind {
            SyntaxKind::Comma => ",",
            SyntaxKind::Pipe => "|",
            _ => panic!(
                "error_skip_separator: unexpected token {:?} (only Comma/Pipe expected)",
                kind
            ),
        };
        if let Some(report) = self.report_at(DiagnosticKind::InvalidSeparator, span) {
            report.fix(format!("remove the `{}`", char_name), "").emit();
        }
        self.bump_as_error();
    }
}

fn regex_literal_is_closed(text: &str) -> bool {
    let Some(body) = text.strip_suffix('/') else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    body.chars().rev().take_while(|&c| c == '\\').count() % 2 == 0
}
