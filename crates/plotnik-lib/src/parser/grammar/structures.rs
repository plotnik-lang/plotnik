use rowan::{Checkpoint, TextRange};

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;
use crate::parser::cst::token_sets::{
    ALT_RECOVERY_TOKENS, EXPR_FIRST_TOKENS, SEPARATORS, SEQ_RECOVERY_TOKENS, TREE_RECOVERY_TOKENS,
};
use crate::parser::cst::{SyntaxKind, TokenSet};
use crate::parser::lexer::token_text;

use super::utils::capitalize_first;

impl Parser<'_, '_> {
    /// `(type ...)` | `(_ ...)` | `(ERROR)` | `(MISSING ...)` | `(RefName)` | `(expr/subtype)`
    /// PascalCase without children → Ref; with children → error but parses as Tree.
    pub(crate) fn parse_tree(&mut self) {
        let checkpoint = self.checkpoint();
        self.push_delimiter(SyntaxKind::ParenOpen);
        let open_paren_span = self.current_span(); // save span before bump
        self.bump(); // consume '('

        let mut is_ref = false;
        let mut ref_name: Option<String> = None;

        match self.current() {
            SyntaxKind::ParenClose => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.diagnostics
                    .report(self.source_id, DiagnosticKind::EmptyTree, open_paren_span)
                    .emit();
                // Fall through to close
            }
            SyntaxKind::Underscore => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.bump();
            }
            SyntaxKind::Id => {
                let (r, n) = self.parse_tree_ref_or_node(checkpoint);
                is_ref = r;
                ref_name = n;
            }
            SyntaxKind::KwError => {
                self.parse_tree_error(checkpoint);
                return;
            }
            SyntaxKind::KwMissing => {
                self.parse_tree_missing(checkpoint);
                return;
            }
            _ => {
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
            }
        }

        self.finish_tree_parsing(checkpoint, is_ref, ref_name);
    }

    fn parse_tree_ref_or_node(&mut self, checkpoint: Checkpoint) -> (bool, Option<String>) {
        let name = token_text(self.source, &self.tokens[self.pos]).to_string();
        let is_pascal_case = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        self.bump();

        let mut is_ref = false;
        let mut ref_name = None;

        if is_pascal_case {
            is_ref = true;
            ref_name = Some(name);
        } else {
            self.start_node_at(checkpoint, SyntaxKind::Tree);
        }

        if self.currently_is(SyntaxKind::Slash) {
            if is_ref {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.error(DiagnosticKind::InvalidSupertypeSyntax);
                is_ref = false;
            }
            self.bump();
            match self.current() {
                SyntaxKind::Id => {
                    self.bump();
                }
                SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                    self.bump_string_tokens();
                }
                _ => {
                    self.error(DiagnosticKind::ExpectedSubtype);
                }
            }
        }
        (is_ref, ref_name)
    }

    fn parse_tree_error(&mut self, checkpoint: Checkpoint) {
        self.start_node_at(checkpoint, SyntaxKind::Tree);
        self.bump(); // KwError
        if !self.currently_is(SyntaxKind::ParenClose) {
            self.error(DiagnosticKind::ErrorTakesNoArguments);
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
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

    fn finish_tree_parsing(
        &mut self,
        checkpoint: Checkpoint,
        is_ref: bool,
        ref_name: Option<String>,
    ) {
        let has_children = !self.currently_is(SyntaxKind::ParenClose);

        if is_ref && has_children {
            self.start_node_at(checkpoint, SyntaxKind::Tree);
            let children_start = self.current_span().start();
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
            let children_end = self.last_non_trivia_end().unwrap_or(children_start);
            let children_span = TextRange::new(children_start, children_end);

            if let Some(name) = &ref_name {
                self.diagnostics
                    .report(
                        self.source_id,
                        DiagnosticKind::RefCannotHaveChildren,
                        children_span,
                    )
                    .message(name)
                    .emit();
            }
        } else if is_ref {
            self.start_node_at(checkpoint, SyntaxKind::Ref);
        } else {
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY_TOKENS);
        }

        self.pop_delimiter();
        self.expect(
            SyntaxKind::ParenClose,
            if is_ref && !has_children {
                "closing ')' for reference"
            } else {
                "closing ')' for tree"
            },
        );
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
                let open = self.delimiter_stack.last().unwrap_or_else(|| {
                    panic!(
                        "parse_children: unclosed {construct} at EOF but delimiter_stack is empty \
                         (caller must push delimiter before calling)"
                    )
                });
                self.error_unclosed_delimiter(kind, format!("{construct} started here"), open.span);
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
            if self.currently_is(SyntaxKind::Predicate) {
                self.error_and_bump(DiagnosticKind::UnsupportedPredicate);
                continue;
            }
            if self.currently_is_one_of(recovery) {
                break;
            }
            self.error_and_bump_with_hint(
                DiagnosticKind::UnexpectedToken,
                "try `(child)` or close with `)`",
            );
        }
    }

    /// Alternation/choice: `[expr1 expr2 ...]` or `[Label: expr ...]`
    pub(crate) fn parse_alt(&mut self) {
        self.start_node(SyntaxKind::Alt);
        self.push_delimiter(SyntaxKind::BracketOpen);
        self.expect(SyntaxKind::BracketOpen, "opening '[' for alternation");

        self.parse_alt_children();

        self.pop_delimiter();
        self.expect(SyntaxKind::BracketClose, "closing ']' for alternation");
        self.finish_node();
    }

    /// Parse alternation children, handling both tagged `Label: expr` and unlabeled expressions.
    fn parse_alt_children(&mut self) {
        loop {
            if self.eof() {
                let open = self.delimiter_stack.last().unwrap_or_else(|| {
                    panic!(
                        "parse_alt_children: unclosed alternation at EOF but delimiter_stack is empty \
                         (caller must push delimiter before calling)"
                    )
                });
                self.error_unclosed_delimiter(
                    DiagnosticKind::UnclosedAlternation,
                    "alternation started here",
                    open.span,
                );
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
                let text = token_text(self.source, &self.tokens[self.pos]);
                let first_char = text.chars().next().unwrap_or('a');
                if first_char.is_ascii_uppercase() {
                    self.parse_branch();
                } else {
                    self.parse_branch_lowercase_label();
                }
                continue;
            }
            // Anchors cannot appear directly in alternations - they create empty branches
            if self.currently_is(SyntaxKind::Dot) {
                self.error(DiagnosticKind::AnchorInAlternation);
                self.skip_token();
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
                "try `(node)` or close with `]`",
            );
        }
    }

    /// Tagged alternation branch: `Label: expr`
    fn parse_branch(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_branch_label(text, span);

        self.expect(SyntaxKind::Colon, "':' after branch label");

        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr();
        } else {
            self.error(DiagnosticKind::ExpectedExpression);
        }

        self.finish_node();
    }

    /// Parse a branch with lowercase label - parse as Branch but emit error.
    fn parse_branch_lowercase_label(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let label_text = token_text(self.source, &self.tokens[self.pos]);
        let capitalized = capitalize_first(label_text);

        self.error_with_fix(
            DiagnosticKind::LowercaseBranchLabel,
            span,
            "branch labels map to enum variants",
            format!("use `{}`", capitalized),
            capitalized,
        );

        self.bump();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        if self.currently_is_one_of(EXPR_FIRST_TOKENS) {
            self.parse_expr();
        } else {
            self.error(DiagnosticKind::ExpectedExpression);
        }

        self.finish_node();
    }

    /// Sibling sequence: `{expr1 expr2 ...}`
    pub(crate) fn parse_seq(&mut self) {
        self.start_node(SyntaxKind::Seq);
        self.push_delimiter(SyntaxKind::BraceOpen);
        self.expect(SyntaxKind::BraceOpen, "opening '{' for sequence");

        self.parse_children(SyntaxKind::BraceClose, SEQ_RECOVERY_TOKENS);

        self.pop_delimiter();
        self.expect(SyntaxKind::BraceClose, "closing '}' for sequence");
        self.finish_node();
    }

    /// Skip a separator token (comma or pipe) and emit helpful error.
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
            format!("plotnik uses whitespace, not `{}`", char_name),
            "remove",
            "",
        );
        self.skip_token();
    }
}
