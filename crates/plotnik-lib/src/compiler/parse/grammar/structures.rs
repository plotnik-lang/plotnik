use rowan::{Checkpoint, TextRange};

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::parse::Parser;
use crate::compiler::parse::cst::SyntaxKind;
use crate::compiler::parse::token_set::{
    ALT_RECOVERY_TOKENS, NODE_RECOVERY_TOKENS, PATTERN_FIRST_TOKENS, PREDICATE_OPS, SEPARATORS,
    SEQ_RECOVERY_TOKENS, TokenSet,
};

use super::utils::{starts_uppercase, to_pascal_case};

enum ParenHead<'q> {
    /// Concrete node kind: `(identifier ...)`.
    Concrete,
    /// PascalCase reference to a named definition: `(Name)`.
    DefRef(&'q str),
}

/// Native node-predicate equivalent for the tree-sitter predicates that have one. The rest
/// (`#any-of?`, `#set!`, `#is?`, …) have no equivalent and get no suggestion.
fn predicate_suggestion(name: &str) -> Option<&'static str> {
    match name {
        "eq" => Some("use `(node == \"x\")`"),
        "not-eq" => Some("use `(node != \"x\")`"),
        "match" => Some("use `(node =~ /re/)`"),
        "not-match" => Some("use `(node !~ /re/)`"),
        _ => None,
    }
}

impl<'q> Parser<'q, '_> {
    /// `(type ...)` | `(_ ...)` | `(ERROR)` | `(MISSING ...)` | `(RefName)`
    /// | `(pattern#subtype)` (native) | `(pattern/subtype)` (tree-sitter compat)
    /// PascalCase without children → DefRef; with children → error but parses as Tree.
    pub(crate) fn parse_named_node(&mut self) {
        let checkpoint = self.checkpoint();
        self.push_delimiter();
        let open_paren_span = self.current_span();
        self.bump();

        let head = match self.current() {
            SyntaxKind::ParenClose => {
                // Empty tree `()` - validation phase will report EmptyTree error
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
                // Tree-sitter style sequence: ((a) (b)) instead of {(a) (b)}
                // Parse as Seq so it works correctly, but warn to encourage {} syntax
                if self.at_ts(PATTERN_FIRST_TOKENS) {
                    self.start_node_at(checkpoint, SyntaxKind::Sequence);
                    if let Some(report) = self.report_at(
                        DiagnosticKind::TreeSitterSequenceSyntaxDeprecated,
                        open_paren_span,
                    ) {
                        report.emit();
                    }
                } else {
                    self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                }
                ParenHead::Concrete
            }
        };

        self.finish_named_node_parsing(checkpoint, head);
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

        // A category separator binds only when tight against the node kind (tree-sitter
        // strictness): `expression#sub` / `expression/sub`, never `expression # sub`.
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
            self.parse_node_predicate();
        }

        head
    }

    /// Parse a category separator and its optional subtype. The CST keeps the marker so later
    /// stages can reject unsupported supertype matching cleanly. Two surface forms, both
    /// tight-binding:
    ///
    /// - `#` — native category syntax. `(name#)` is a bare category; `(name#sub)` refines it.
    /// - `/` — tree-sitter compatibility. Always needs a subtype; warns toward `#`.
    ///
    /// The separator is already known to be tight against the node kind; here we also require
    /// the subtype to be tight against the separator, matching tree-sitter exactly.
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
            report.emit();
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
        self.bump(); // consume `/` or `#`
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

    /// A tree-sitter predicate like `#eq?` or `#set!`: `#` tight against an identifier, itself
    /// tight against a `?`/`!` suffix. Real predicates always carry that suffix, so requiring it
    /// keeps a bare `#node_type` (a loose category-refinement typo) out of predicate recovery —
    /// it falls through to the same "wrap in parens" diagnosis as the spaced form. A lone or
    /// loosely-spaced `#` also falls through, so the caller keeps its "unexpected token" hint.
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

    /// A misplaced tree-sitter predicate (`#eq?`, `#match?`, `#set!`) — unsupported. Consume the
    /// whole tight `#name[?!]` run into one Error node so it reports as a single unit instead of
    /// cascading into a bogus node and quantifier. Only call when [`at_ts_predicate`] holds.
    pub(crate) fn error_unsupported_predicate(&mut self) {
        self.start_node(SyntaxKind::Error);
        let (span, name) = self.consume_predicate_name();
        self.finish_node();

        self.report_unsupported_predicate(span, name);
    }

    /// Consume a tight predicate run `#name?` / `#name!`, returning its full span and the bare
    /// predicate name (`eq`, `not-eq`, `match`, …). It lexes as `#` + identifier + `?`/`!`; the
    /// suffix is guaranteed by the [`at_ts_predicate`] precondition. Only call when it holds.
    fn consume_predicate_name(&mut self) -> (TextRange, &'q str) {
        let start = self.current_span().start();
        self.bump(); // `#`
        let name = self.current_text(); // identifier, tight by precondition
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

    /// A parenthesized predicate written tree-sitter style, e.g. `(#eq? @x "foo")`. The `(` is
    /// already consumed; swallow the predicate name and its arguments through the predicate's own
    /// closing `)` into one Error node, so the arguments don't cascade into bogus child
    /// diagnostics. Only call when [`at_ts_predicate`] holds.
    fn parse_node_predicate_error(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Error);
        let (span, name) = self.consume_predicate_name();
        self.report_unsupported_predicate(span, name);
        self.consume_until_matching_paren();
        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
    }

    fn consume_until_matching_paren(&mut self) {
        // Swallow arguments up to the predicate's own closing `)`, tracking delimiter depth: a
        // nested argument like `(identifier)` ends at its own `)`, while an enclosing `]`/`}`
        // left by a missing `)` stops the run *without* being consumed — so the outer
        // alternation/sequence still closes and only the local missing `)` is reported.
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

    /// Report an unsupported tree-sitter predicate, suggesting the native node predicate only
    /// for the ones with a real equivalent. `#set!`/`#is?`/`#any-of?` have none, so they get
    /// the bare "not supported" message rather than a misleading suggestion.
    fn report_unsupported_predicate(&mut self, span: TextRange, name: &str) {
        let Some(report) = self.report_at(DiagnosticKind::UnsupportedPredicate, span) else {
            return;
        };

        match predicate_suggestion(name) {
            Some(hint) => report.hint(hint).emit(),
            None => report.emit(),
        }
    }

    /// Parse a node predicate: `== "value"`, `=~ /pattern/`, etc.
    fn parse_node_predicate(&mut self) {
        self.start_node(SyntaxKind::NodePredicate);

        self.bump();

        match self.current() {
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                self.skip_string_tokens();
            }
            SyntaxKind::UnterminatedString => {
                self.error_and_bump(DiagnosticKind::UnclosedString);
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
                // Standalone slash - parse as regex (fallback, shouldn't happen normally)
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

    /// Parse a regex literal: `/pattern/`
    ///
    /// Regex literals consume all content verbatim (including whitespace and
    /// comment-like sequences) until an unescaped closing `/` is found.
    fn parse_regex_literal(&mut self) {
        self.start_node(SyntaxKind::Regex);
        self.bump(); // opening '/'

        let mut found_close = false;

        while !self.eof() && !self.has_fatal_error() {
            let kind = self.nth_raw(0);

            // Inside regex, include ALL tokens (trivia too)
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
            self.bump(); // closing '/'
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
        if !self.at(SyntaxKind::ParenClose) {
            let children_start = self.current_span().start();
            self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
            let children_end = self.last_non_trivia_end().unwrap_or(children_start);
            let children_span = TextRange::new(children_start, children_end);
            if let Some(report) =
                self.report_at(DiagnosticKind::ErrorTakesNoArguments, children_span)
            {
                report.fix("remove the children", "").emit();
            }
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
            SyntaxKind::ParenClose => {}
            _ => {
                self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
            }
        }
        self.pop_delimiter();
        self.expect_close(SyntaxKind::ParenClose, DiagnosticKind::UnclosedTree);
        self.finish_node();
    }

    fn finish_named_node_parsing(&mut self, checkpoint: Checkpoint, head: ParenHead<'q>) {
        let has_children = !self.at(SyntaxKind::ParenClose);

        match head {
            ParenHead::DefRef(name) if has_children => {
                self.start_node_at(checkpoint, SyntaxKind::NamedNode);
                let children_start = self.current_span().start();
                self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY_TOKENS);
                let children_end = self.last_non_trivia_end().unwrap_or(children_start);
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
            if self.at_ts(SEPARATORS) {
                self.error_skip_separator();
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

    /// Alternation/choice: `[expr1 expr2 ...]` or `[Label: pattern ...]`
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
            if self.at_ts(SEPARATORS) {
                self.error_skip_separator();
                continue;
            }

            // LL(2): Id followed by Colon → branch label or field (check casing)
            if self.at(SyntaxKind::Id) && self.next_is(SyntaxKind::Colon) {
                if starts_uppercase(self.current_text()) {
                    self.parse_branch();
                } else {
                    self.parse_branch_lowercase_label();
                }
                continue;
            }
            // Anchors cannot appear directly in alternations - they create empty branches
            if matches!(self.current(), SyntaxKind::Dot | SyntaxKind::DotBang) {
                self.error_and_bump(DiagnosticKind::AnchorInAlternation);
                continue;
            }
            if self.at_ts_predicate() {
                self.error_unsupported_predicate();
                continue;
            }
            if self.at_ts(PATTERN_FIRST_TOKENS) {
                self.start_node(SyntaxKind::Branch);
                self.parse_pattern();
                self.finish_node();
                continue;
            }
            if self.at_ts(ALT_RECOVERY_TOKENS) {
                break;
            }
            self.report_unexpected_branch_run();
        }
    }

    fn report_unexpected_branch_run(&mut self) {
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
            report.detail("expected a branch, or `]` to close").emit();
        }
    }

    /// Consume adjacent garbage as one error node so recovery reports one diagnostic per run.
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
            // Lookahead probes can drain trailing trivia; check EOF after them.
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

    /// Enum branch: `Label: pattern`
    fn parse_branch(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let ident = self.bump_ident();
        self.validate_branch_label(ident);

        self.expect(SyntaxKind::Colon, "':' after branch label");

        self.parse_required_pattern();

        self.finish_node();
    }

    /// Parse a branch with lowercase label - parse as Branch but emit error.
    fn parse_branch_lowercase_label(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let label_text = self.current_text();
        let pascal = to_pascal_case(label_text);

        if let Some(report) = self.report_at(DiagnosticKind::BranchLabelInvalid, span) {
            report.fix(format!("use `{}`", pascal), pascal).emit();
        }

        self.bump();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        self.parse_required_pattern();

        self.finish_node();
    }

    /// Sibling sequence: `{expr1 expr2 ...}`
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

    /// Consume a separator token (comma or pipe) into an Error node and emit a helpful error.
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

/// A `RegexLiteral` token is `/`-delimited; the lexer stops at the line end
/// when the closing `/` is missing, so closedness must be re-checked here.
fn regex_literal_is_closed(text: &str) -> bool {
    let Some(body) = text.strip_suffix('/') else {
        return false;
    };
    if body.is_empty() {
        // A lone `/` is only an opener.
        return false;
    }
    body.chars().rev().take_while(|&c| c == '\\').count() % 2 == 0
}
