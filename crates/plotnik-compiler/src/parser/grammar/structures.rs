use rowan::{Checkpoint, TextRange};

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::token_sets::{
    ALT_RECOVERY_TOKENS, EXPR_FIRST_TOKENS, PREDICATE_OPS, SEPARATORS, SEQ_RECOVERY_TOKENS,
    TREE_RECOVERY_TOKENS,
};
use crate::parser::cst::{SyntaxKind, TokenSet};

use super::utils::{starts_uppercase, to_pascal_case};

/// What the identifier after `(` turned out to be.
enum TreeHead<'q> {
    /// Concrete node type: `(identifier ...)`.
    Node,
    /// PascalCase reference to a named definition: `(Name)`.
    Ref(&'q str),
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
    /// | `(expr#subtype)` (native) | `(expr/subtype)` (tree-sitter compat)
    /// PascalCase without children → Ref; with children → error but parses as Tree.
    pub(crate) fn parse_tree(&mut self) {
        let checkpoint = self.checkpoint();
        self.push_delimiter();
        let open_paren_span = self.current_span(); // save span before bump
        self.bump(); // consume '('

        let head = match self.current() {
            SyntaxKind::ParenClose => {
                // Empty tree `()` - validation phase will report EmptyTree error
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                TreeHead::Node
            }
            SyntaxKind::Underscore => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.bump();
                TreeHead::Node
            }
            SyntaxKind::Id => self.parse_tree_ref_or_node(checkpoint),
            SyntaxKind::KwError => {
                self.parse_tree_error(checkpoint);
                return;
            }
            SyntaxKind::KwMissing => {
                self.parse_tree_missing(checkpoint);
                return;
            }
            _ => {
                if self.at_ts_predicate() {
                    self.parse_tree_predicate(checkpoint);
                    return;
                }
                // Tree-sitter style sequence: ((a) (b)) instead of {(a) (b)}
                // Parse as Seq so it works correctly, but warn to encourage {} syntax
                if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
                    self.start_node_at(checkpoint, SyntaxKind::Seq);
                    self.diagnostics
                        .report(
                            self.source_id,
                            DiagnosticKind::TreeSitterSequenceSyntax,
                            open_paren_span,
                        )
                        .emit();
                } else {
                    self.start_node_at(checkpoint, SyntaxKind::Tree);
                }
                TreeHead::Node
            }
        };

        self.finish_tree_parsing(checkpoint, head);
    }

    fn parse_tree_ref_or_node(&mut self, checkpoint: Checkpoint) -> TreeHead<'q> {
        let name = self.current_text();
        let head_end = self.current_span().end();
        self.bump();

        let mut head = if starts_uppercase(name) {
            TreeHead::Ref(name)
        } else {
            self.start_node_at(checkpoint, SyntaxKind::Tree);
            TreeHead::Node
        };

        // A category separator binds only when tight against the node type (tree-sitter
        // strictness): `expression#sub` / `expression/sub`, never `expression # sub`.
        if let Some(sep) = self.tokens.get(self.pos).copied()
            && matches!(sep.kind, SyntaxKind::Slash | SyntaxKind::Hash)
            && sep.span.start() == head_end
        {
            self.parse_supertype(checkpoint, &mut head, sep.kind);
        }

        // Parse optional predicate: `(identifier == "foo")` or `(identifier =~ /pattern/)`
        if matches!(head, TreeHead::Node) && self.currently_is_one_of(PREDICATE_OPS) {
            self.parse_node_predicate();
        }

        head
    }

    /// Parse a category separator and its optional subtype, recorded in the CST but ignored
    /// downstream (no subtype matching yet). Two surface forms, both tight-binding:
    ///
    /// - `#` — native category syntax. `(name#)` is a bare category; `(name#sub)` refines it.
    /// - `/` — tree-sitter compatibility. Always needs a subtype; warns toward `#`.
    ///
    /// The separator is already known to be tight against the node type; here we also require
    /// the subtype to be tight against the separator, matching tree-sitter exactly.
    fn parse_supertype(
        &mut self,
        checkpoint: Checkpoint,
        head: &mut TreeHead<'q>,
        sep: SyntaxKind,
    ) {
        let is_ref = matches!(head, TreeHead::Ref(_));
        if is_ref {
            self.start_node_at(checkpoint, SyntaxKind::Tree);
            self.error(DiagnosticKind::InvalidSupertypeSyntax);
            *head = TreeHead::Node;
        }

        let sep_span = self.current_span();
        self.bump(); // consume `/` or `#`

        let subtype = self.tokens.get(self.pos).copied();
        let tight = subtype.is_some_and(|t| t.span.start() == sep_span.end());
        let has_subtype = match subtype.map(|t| t.kind) {
            Some(SyntaxKind::Id) if tight => {
                self.bump();
                true
            }
            Some(SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote) if tight => {
                self.bump_string_tokens();
                true
            }
            _ => false,
        };

        if sep == SyntaxKind::Slash {
            if has_subtype && !is_ref {
                self.error_with_fix(
                    DiagnosticKind::SupertypeSlashDeprecated,
                    sep_span,
                    "use `#`",
                    "#",
                );
            } else if !has_subtype {
                self.error(DiagnosticKind::ExpectedSubtype);
            }
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
    fn parse_tree_predicate(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Error);
        let (span, name) = self.consume_predicate_name();
        self.report_unsupported_predicate(span, name);
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
        self.pop_delimiter();
        self.expect(SyntaxKind::ParenClose, "closing ')' for predicate");
        self.finish_node();
    }

    /// Report an unsupported tree-sitter predicate, suggesting the native node predicate only
    /// for the ones with a real equivalent. `#set!`/`#is?`/`#any-of?` have none, so they get
    /// the bare "not supported" message rather than a misleading suggestion.
    fn report_unsupported_predicate(&mut self, span: TextRange, name: &str) {
        match predicate_suggestion(name) {
            Some(hint) => self.error_at_with_hint(DiagnosticKind::UnsupportedPredicate, span, hint),
            None => self.error_at(DiagnosticKind::UnsupportedPredicate, span),
        }
    }

    /// Parse a node predicate: `== "value"`, `=~ /pattern/`, etc.
    fn parse_node_predicate(&mut self) {
        self.start_node(SyntaxKind::NodePredicate);

        // Consume the operator
        self.bump();

        // Parse the value (string or regex)
        match self.current() {
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                self.bump_string_tokens();
            }
            SyntaxKind::UnterminatedString => {
                self.error_and_bump(DiagnosticKind::UnclosedString);
            }
            SyntaxKind::RegexLiteral => {
                // Regex literal from compound token - wrap in Regex node
                self.start_node(SyntaxKind::Regex);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::Slash => {
                // Standalone slash - parse as regex (fallback, shouldn't happen normally)
                self.parse_regex_literal();
            }
            _ => {
                self.error(DiagnosticKind::ExpectedPredicateValue);
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

            // Check for escaped slash by counting trailing backslashes in source
            let slash_start: usize = self.tokens[self.pos].span.start().into();
            let backslash_count = self.source[..slash_start]
                .chars()
                .rev()
                .take_while(|&c| c == '\\')
                .count();

            // Odd number of backslashes means the slash is escaped
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
            self.error(DiagnosticKind::UnclosedRegex);
        }

        self.finish_node();
    }

    fn parse_tree_error(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Tree);
        self.bump(); // KwError
        if !self.currently_is(SyntaxKind::ParenClose) {
            let children_start = self.current_span().start();
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
            let children_end = self.last_non_trivia_end().unwrap_or(children_start);
            let children_span = TextRange::new(children_start, children_end);
            self.diagnostics
                .report(
                    self.source_id,
                    DiagnosticKind::ErrorTakesNoArguments,
                    children_span,
                )
                .fix("remove the children", "")
                .emit();
        }
        self.pop_delimiter();
        self.expect(SyntaxKind::ParenClose, "closing ')' for (ERROR)");
        self.finish_node();
    }

    fn parse_tree_missing(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Tree);
        self.bump(); // KwMissing
        match self.current() {
            SyntaxKind::Id => {
                self.bump();
            }
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                self.bump_string_tokens();
            }
            SyntaxKind::ParenClose => {}
            _ => {
                self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
            }
        }
        self.pop_delimiter();
        self.expect(SyntaxKind::ParenClose, "closing ')' for (MISSING)");
        self.finish_node();
    }

    fn finish_tree_parsing(&mut self, checkpoint: Checkpoint, head: TreeHead<'q>) {
        let has_children = !self.currently_is(SyntaxKind::ParenClose);

        let what = match head {
            TreeHead::Ref(name) if has_children => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                let children_start = self.current_span().start();
                self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
                let children_end = self.last_non_trivia_end().unwrap_or(children_start);
                let children_span = TextRange::new(children_start, children_end);

                self.diagnostics
                    .report(
                        self.source_id,
                        DiagnosticKind::RefCannotHaveChildren,
                        children_span,
                    )
                    .message(name)
                    .fix("remove the children", "")
                    .emit();
                "closing ')' for tree"
            }
            TreeHead::Ref(_) => {
                self.start_node_at(checkpoint, SyntaxKind::Ref);
                "closing ')' for reference"
            }
            TreeHead::Node => {
                self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
                "closing ')' for tree"
            }
        };

        self.pop_delimiter();
        self.expect(SyntaxKind::ParenClose, what);
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
            if self.currently_is(until) {
                break;
            }
            if self.currently_is_one_of(SEPARATORS) {
                self.error_skip_separator();
                continue;
            }
            if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
                self.parse_expr();
                continue;
            }
            if self.at_ts_predicate() {
                self.error_unsupported_predicate();
                continue;
            }
            if self.currently_is_one_of(recovery) {
                break;
            }
            self.error_and_bump_with_hint(
                DiagnosticKind::UnexpectedToken,
                "expected a child node, or `)` to close",
            );
        }
    }

    /// Alternation/choice: `[expr1 expr2 ...]` or `[Label: expr ...]`
    pub(crate) fn parse_alt(&mut self) {
        self.start_node(SyntaxKind::Alt);
        self.push_delimiter();
        self.assert_current(SyntaxKind::BracketOpen);
        self.bump(); // consume '['

        self.parse_alt_children();

        self.pop_delimiter();
        self.expect(SyntaxKind::BracketClose, "closing ']' for alternation");
        self.finish_node();
    }

    /// Parse alternation children, handling both tagged `Label: expr` and unlabeled expressions.
    fn parse_alt_children(&mut self) {
        loop {
            if self.eof() {
                self.error_unclosed_at_eof(DiagnosticKind::UnclosedAlternation, "alternation");
                break;
            }
            if self.has_fatal_error() {
                break;
            }
            if self.currently_is(SyntaxKind::BracketClose) {
                break;
            }
            if self.currently_is_one_of(SEPARATORS) {
                self.error_skip_separator();
                continue;
            }

            // LL(2): Id followed by Colon → branch label or field (check casing)
            if self.currently_is(SyntaxKind::Id) && self.next_is(SyntaxKind::Colon) {
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
            if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
                self.start_node(SyntaxKind::Branch);
                self.parse_expr();
                self.finish_node();
                continue;
            }
            if self.currently_is_one_of(ALT_RECOVERY_TOKENS) {
                break;
            }
            self.error_and_bump_with_hint(
                DiagnosticKind::UnexpectedToken,
                "expected a branch, or `]` to close",
            );
        }
    }

    /// Tagged alternation branch: `Label: expr`
    fn parse_branch(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let text = self.current_text();
        self.bump();
        self.validate_branch_label(text, span);

        self.expect(SyntaxKind::Colon, "':' after branch label");

        self.parse_required_expr();

        self.finish_node();
    }

    /// Parse a branch with lowercase label - parse as Branch but emit error.
    fn parse_branch_lowercase_label(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let label_text = self.current_text();
        let pascal = to_pascal_case(label_text);

        self.error_with_fix(
            DiagnosticKind::BranchLabelInvalid,
            span,
            format!("use `{}`", pascal),
            pascal,
        );

        self.bump();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        self.parse_required_expr();

        self.finish_node();
    }

    /// Sibling sequence: `{expr1 expr2 ...}`
    pub(crate) fn parse_seq(&mut self) {
        self.start_node(SyntaxKind::Seq);
        self.push_delimiter();
        self.assert_current(SyntaxKind::BraceOpen);
        self.bump(); // consume '{'

        self.parse_children(SyntaxKind::BraceClose, SEQ_RECOVERY_TOKENS);

        self.pop_delimiter();
        self.expect(SyntaxKind::BraceClose, "closing '}' for sequence");
        self.finish_node();
    }

    /// Consume a separator token (comma or pipe) into an Error node and emit a helpful error.
    fn error_skip_separator(&mut self) {
        let kind = self.current();
        let span = self.current_span();
        // Invariant: only called when SEPARATORS.contains(kind), which only has Comma and Pipe
        let char_name = match kind {
            SyntaxKind::Comma => ",",
            SyntaxKind::Pipe => "|",
            _ => panic!(
                "error_skip_separator: unexpected token {:?} (only Comma/Pipe expected)",
                kind
            ),
        };
        self.error_with_fix(
            DiagnosticKind::InvalidSeparator,
            span,
            format!("remove the `{}`", char_name),
            "",
        );
        self.bump_as_error();
    }
}
