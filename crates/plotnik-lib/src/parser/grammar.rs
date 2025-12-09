//! Grammar productions for the query language.
//!
//! This module implements all `parse_*` methods as an extension of `Parser`.
//! The grammar follows tree-sitter query syntax with extensions for named subqueries.

use rowan::{Checkpoint, TextRange};

use super::core::Parser;

use super::cst::token_sets::{
    ALT_RECOVERY, EXPR_FIRST, QUANTIFIERS, SEPARATORS, SEQ_RECOVERY, TREE_RECOVERY,
};
use super::cst::{SyntaxKind, TokenSet};
use super::lexer::token_text;
use crate::diagnostics::DiagnosticKind;

impl Parser<'_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        let mut unnamed_def_spans: Vec<TextRange> = Vec::new();

        while !self.has_fatal_error() && (self.peek() != SyntaxKind::Error || !self.eof()) {
            // LL(2): Id followed by Equals → named definition (if PascalCase)
            if self.peek() == SyntaxKind::Id && self.peek_nth(1) == SyntaxKind::Equals {
                self.parse_def();
            } else {
                let start = self.current_span().start();
                self.start_node(SyntaxKind::Def);
                let success = self.parse_expr_or_error();
                if !success {
                    self.synchronize_to_def_start();
                }
                self.finish_node();
                if success {
                    let end = self.last_non_trivia_end().unwrap_or(start);
                    unnamed_def_spans.push(TextRange::new(start, end));
                }
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

    /// Named expression definition: `Name = expr`
    fn parse_def(&mut self) {
        self.start_node(SyntaxKind::Def);

        let span = self.current_span();
        let name = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_def_name(name, span);

        self.peek();
        let ate_equals = self.eat(SyntaxKind::Equals);
        assert!(
            ate_equals,
            "parse_def: expected '=' but found {:?} (caller should verify Equals is present)",
            self.current()
        );

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error_msg(
                DiagnosticKind::ExpectedExpression,
                "after `=` in definition",
            );
        }

        self.finish_node();
    }

    /// Parse an expression, or emit an error if current token can't start one.
    /// Returns `true` if a valid expression was parsed, `false` on error.
    fn parse_expr_or_error(&mut self) -> bool {
        let kind = self.peek();
        if EXPR_FIRST.contains(kind) {
            self.parse_expr();
            true
        } else if kind == SyntaxKind::At {
            self.error_and_bump(DiagnosticKind::CaptureWithoutTarget);
            false
        } else if kind == SyntaxKind::Predicate {
            self.error_and_bump(DiagnosticKind::UnsupportedPredicate);
            false
        } else {
            self.error_and_bump_msg(
                DiagnosticKind::UnexpectedToken,
                "try `(node)`, `[a b]`, `{a b}`, `\"literal\"`, or `_`",
            );
            false
        }
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier/capture suffix.
    fn parse_expr(&mut self) {
        self.parse_expr_inner(true)
    }

    /// Parse expression without applying quantifier/capture suffix.
    /// Used for field values so that `field: (x)*` parses as `(field: (x))*`.
    fn parse_expr_no_suffix(&mut self) {
        self.parse_expr_inner(false)
    }

    fn parse_expr_inner(&mut self, with_suffix: bool) {
        if !self.enter_recursion() {
            self.start_node(SyntaxKind::Error);
            while !self.should_stop() {
                self.bump();
            }
            self.finish_node();
            return;
        }

        let checkpoint = self.checkpoint();

        match self.peek() {
            SyntaxKind::ParenOpen => self.parse_tree(),
            SyntaxKind::BracketOpen => self.parse_alt(),
            SyntaxKind::BraceOpen => self.parse_seq(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => self.parse_str(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation => self.parse_negated_field(),
            SyntaxKind::Id => self.parse_tree_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(DiagnosticKind::ErrorMissingOutsideParens);
            }
            _ => {
                self.error_and_bump_msg(DiagnosticKind::UnexpectedToken, "not a valid expression");
            }
        }

        if with_suffix {
            self.try_parse_quantifier(checkpoint);
            self.try_parse_capture(checkpoint);
        }

        self.exit_recursion();
    }

    /// `(type ...)` | `(_ ...)` | `(ERROR)` | `(MISSING ...)` | `(RefName)` | `(expr/subtype)`
    /// PascalCase without children → Ref; with children → error but parses as Tree.
    fn parse_tree(&mut self) {
        let checkpoint = self.checkpoint();
        self.push_delimiter(SyntaxKind::ParenOpen);
        self.bump(); // consume '('

        let mut is_ref = false;
        let mut ref_name: Option<String> = None;

        match self.peek() {
            SyntaxKind::ParenClose => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.error(DiagnosticKind::EmptyTree);
                self.pop_delimiter();
                self.bump(); // consume ')'
                self.finish_node();
                return;
            }
            SyntaxKind::Underscore => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.bump();
            }
            SyntaxKind::Id => {
                let name = token_text(self.source, &self.tokens[self.pos]).to_string();
                let is_pascal_case = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                self.bump();

                if is_pascal_case {
                    is_ref = true;
                    ref_name = Some(name);
                } else {
                    self.start_node_at(checkpoint, SyntaxKind::Tree);
                }

                if self.peek() == SyntaxKind::Slash {
                    if is_ref {
                        self.start_node_at(checkpoint, SyntaxKind::Tree);
                        self.error(DiagnosticKind::InvalidSupertypeSyntax);
                        is_ref = false;
                    }
                    self.bump();
                    match self.peek() {
                        SyntaxKind::Id => {
                            self.bump();
                        }
                        SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                            self.bump_string_tokens();
                        }
                        _ => {
                            self.error_msg(
                                DiagnosticKind::ExpectedSubtype,
                                "e.g., `expression/binary_expression`",
                            );
                        }
                    }
                }
            }
            SyntaxKind::KwError => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.bump();
                if self.peek() != SyntaxKind::ParenClose {
                    self.error(DiagnosticKind::ErrorTakesNoArguments);
                    self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
                }
                self.pop_delimiter();
                self.expect(SyntaxKind::ParenClose, "closing ')' for (ERROR)");
                self.finish_node();
                return;
            }
            SyntaxKind::KwMissing => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
                self.bump();
                match self.peek() {
                    SyntaxKind::Id => {
                        self.bump();
                    }
                    SyntaxKind::SingleQuote | SyntaxKind::DoubleQuote => {
                        self.bump_string_tokens();
                    }
                    SyntaxKind::ParenClose => {}
                    _ => {
                        self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
                    }
                }
                self.pop_delimiter();
                self.expect(SyntaxKind::ParenClose, "closing ')' for (MISSING)");
                self.finish_node();
                return;
            }
            _ => {
                self.start_node_at(checkpoint, SyntaxKind::Tree);
            }
        }

        let has_children = self.peek() != SyntaxKind::ParenClose;

        if is_ref && has_children {
            self.start_node_at(checkpoint, SyntaxKind::Tree);
            let children_start = self.current_span().start();
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
            let children_end = self.last_non_trivia_end().unwrap_or(children_start);
            let children_span = TextRange::new(children_start, children_end);

            if let Some(name) = &ref_name {
                self.diagnostics
                    .report(DiagnosticKind::RefCannotHaveChildren, children_span)
                    .message(name)
                    .emit();
            }
        } else if is_ref {
            self.start_node_at(checkpoint, SyntaxKind::Ref);
        } else {
            self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
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

    /// Parse children until `until` token or recovery set hit.
    fn parse_children(&mut self, until: SyntaxKind, recovery: TokenSet) {
        loop {
            if self.eof() {
                let (construct, delim, kind) = match until {
                    SyntaxKind::ParenClose => ("tree", "`)`", DiagnosticKind::UnclosedTree),
                    SyntaxKind::BraceClose => ("sequence", "`}`", DiagnosticKind::UnclosedSequence),
                    _ => panic!(
                        "parse_children: unexpected delimiter {:?} (only ParenClose/BraceClose supported)",
                        until
                    ),
                };
                let msg = format!("expected {delim}");
                let open = self.delimiter_stack.last().unwrap_or_else(|| {
                    panic!(
                        "parse_children: unclosed {construct} at EOF but delimiter_stack is empty \
                         (caller must push delimiter before calling)"
                    )
                });
                self.error_unclosed_delimiter(
                    kind,
                    msg,
                    format!("{construct} started here"),
                    open.span,
                );
                break;
            }
            if self.has_fatal_error() {
                break;
            }
            let kind = self.peek();
            if kind == until {
                break;
            }
            if SEPARATORS.contains(kind) {
                self.error_skip_separator();
                continue;
            }
            if EXPR_FIRST.contains(kind) {
                self.parse_expr();
                continue;
            }
            if kind == SyntaxKind::Predicate {
                self.error_and_bump(DiagnosticKind::UnsupportedPredicate);
                continue;
            }
            if recovery.contains(kind) {
                break;
            }
            self.error_and_bump_msg(
                DiagnosticKind::UnexpectedToken,
                "not valid inside a node — try `(child)` or close with `)`",
            );
        }
    }

    /// Alternation/choice: `[expr1 expr2 ...]` or `[Label: expr ...]`
    fn parse_alt(&mut self) {
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
                let msg = "expected `]`";
                let open = self.delimiter_stack.last().unwrap_or_else(|| {
                    panic!(
                        "parse_alt_children: unclosed alternation at EOF but delimiter_stack is empty \
                         (caller must push delimiter before calling)"
                    )
                });
                self.error_unclosed_delimiter(
                    DiagnosticKind::UnclosedAlternation,
                    msg,
                    "alternation started here",
                    open.span,
                );
                break;
            }
            if self.has_fatal_error() {
                break;
            }
            let kind = self.peek();
            if kind == SyntaxKind::BracketClose {
                break;
            }
            if SEPARATORS.contains(kind) {
                self.error_skip_separator();
                continue;
            }

            // LL(2): Id followed by Colon → branch label or field (check casing)
            if kind == SyntaxKind::Id && self.peek_nth(1) == SyntaxKind::Colon {
                let text = token_text(self.source, &self.tokens[self.pos]);
                let first_char = text.chars().next().unwrap_or('a');
                if first_char.is_ascii_uppercase() {
                    self.parse_branch();
                } else {
                    self.parse_branch_lowercase_label();
                }
                continue;
            }
            if EXPR_FIRST.contains(kind) {
                self.start_node(SyntaxKind::Branch);
                self.parse_expr();
                self.finish_node();
                continue;
            }
            if ALT_RECOVERY.contains(kind) {
                break;
            }
            self.error_and_bump_msg(
                DiagnosticKind::UnexpectedToken,
                "not valid inside alternation — try `(node)` or close with `]`",
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

        self.peek();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `Label:`");
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
        self.peek();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `label:`");
        }

        self.finish_node();
    }

    /// Sibling sequence: `{expr1 expr2 ...}`
    fn parse_seq(&mut self) {
        self.start_node(SyntaxKind::Seq);
        self.push_delimiter(SyntaxKind::BraceOpen);
        self.expect(SyntaxKind::BraceOpen, "opening '{' for sequence");

        self.parse_children(SyntaxKind::BraceClose, SEQ_RECOVERY);

        self.pop_delimiter();
        self.expect(SyntaxKind::BraceClose, "closing '}' for sequence");
        self.finish_node();
    }

    fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore, "'_' wildcard");
        self.finish_node();
    }

    /// `"if"` | `'+'`
    fn parse_str(&mut self) {
        self.start_node(SyntaxKind::Str);
        self.bump_string_tokens();
        self.finish_node();
    }

    /// Consume string tokens (quote + optional content + quote) without creating a node.
    /// Used for contexts where string appears as a raw value (supertype, MISSING arg).
    fn bump_string_tokens(&mut self) {
        let open_quote = self.peek();
        self.bump(); // opening quote

        if self.peek() == SyntaxKind::StrVal {
            self.bump(); // content
        }

        let closing = self.peek();
        assert_eq!(
            closing, open_quote,
            "bump_string_tokens: expected closing {:?} but found {:?} \
             (lexer should only produce quote tokens from complete strings)",
            open_quote, closing
        );
        self.bump();
    }

    /// `@name` | `@name :: Type`
    fn parse_capture_suffix(&mut self) {
        self.bump(); // consume At

        if self.peek() != SyntaxKind::Id {
            self.error(DiagnosticKind::ExpectedCaptureName);
            return;
        }

        let span = self.current_span();
        let name = token_text(self.source, &self.tokens[self.pos]);
        self.bump(); // consume Id

        self.validate_capture_name(name, span);

        if self.peek() == SyntaxKind::Colon {
            self.parse_type_annotation_single_colon();
            return;
        }
        if self.peek() == SyntaxKind::DoubleColon {
            self.parse_type_annotation();
        }
    }

    /// Type annotation: `::Type` (PascalCase) or `::string` (primitive)
    fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon, "'::' for type annotation");

        if self.peek() == SyntaxKind::Id {
            let span = self.current_span();
            let text = token_text(self.source, &self.tokens[self.pos]);
            self.bump();
            self.validate_type_name(text, span);
        } else {
            self.error_msg(
                DiagnosticKind::ExpectedTypeName,
                "e.g., `::MyType` or `::string`",
            );
        }

        self.finish_node();
    }

    /// Handle single colon type annotation (common mistake: `@x : Type` instead of `@x :: Type`)
    fn parse_type_annotation_single_colon(&mut self) {
        if self.peek_nth(1) != SyntaxKind::Id {
            return;
        }

        self.start_node(SyntaxKind::Type);

        let span = self.current_span();
        self.error_with_fix(
            DiagnosticKind::InvalidTypeAnnotationSyntax,
            span,
            "single `:` looks like a field",
            "use `::`",
            "::",
        );

        self.bump(); // colon

        // peek() skips whitespace, so this handles `@x : Type` with space
        if self.peek() == SyntaxKind::Id {
            self.bump();
        }

        self.finish_node();
    }

    /// `.` anchor
    fn parse_anchor(&mut self) {
        self.start_node(SyntaxKind::Anchor);
        self.expect(SyntaxKind::Dot, "'.' anchor");
        self.finish_node();
    }

    /// Negated field assertion: `!field` (field must be absent)
    fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation, "'!' for negated field");

        if self.peek() != SyntaxKind::Id {
            self.error_msg(DiagnosticKind::ExpectedFieldName, "e.g., `!value`");
            self.finish_node();
            return;
        }

        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_field_name(text, span);
        self.finish_node();
    }

    /// Disambiguate `field: expr` from bare identifier via LL(2) lookahead.
    /// Also handles `field = expr` typo (should be `field: expr`).
    fn parse_tree_or_field(&mut self) {
        if self.peek_nth(1) == SyntaxKind::Colon {
            self.parse_field();
        } else if self.peek_nth(1) == SyntaxKind::Equals {
            self.parse_field_equals_typo();
        } else {
            // Bare identifiers are not valid expressions; trees require parentheses
            self.error_and_bump_msg(
                DiagnosticKind::BareIdentifier,
                "wrap in parentheses: `(identifier)`",
            );
        }
    }

    /// Field constraint: `field_name: expr`
    fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        let kind = self.peek();
        assert_eq!(
            kind,
            SyntaxKind::Id,
            "parse_field: expected Id but found {:?} (caller should verify Id is present)",
            kind
        );
        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
        self.bump();
        self.validate_field_name(text, span);

        self.expect(
            SyntaxKind::Colon,
            "':' to separate field name from its value",
        );

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr_no_suffix();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `field:`");
        }

        self.finish_node();
    }

    /// Handle `field = expr` typo - parse as Field but emit error.
    fn parse_field_equals_typo(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.bump();
        self.peek();
        let span = self.current_span();
        self.error_with_fix(
            DiagnosticKind::InvalidFieldEquals,
            span,
            "this isn't a definition",
            "use `:`",
            ":",
        );
        self.bump();

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error_msg(DiagnosticKind::ExpectedExpression, "after `field =`");
        }

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

    /// If current token is quantifier, wrap preceding expression using checkpoint.
    fn try_parse_quantifier(&mut self, checkpoint: Checkpoint) {
        if self.at_set(QUANTIFIERS) {
            self.start_node_at(checkpoint, SyntaxKind::Quantifier);
            self.bump();
            self.finish_node();
        }
    }

    /// If current token is a capture (`@name`), wrap preceding expression with Capture using checkpoint.
    fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
        if self.peek() == SyntaxKind::At {
            self.start_node_at(checkpoint, SyntaxKind::Capture);
            self.parse_capture_suffix();
            self.finish_node();
        }
    }

    /// Validate capture name follows plotnik convention (snake_case).
    fn validate_capture_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') {
            let suggested = name.replace(['.', '-'], "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::CaptureNameHasDots,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
            return;
        }

        if name.contains('-') {
            let suggested = name.replace('-', "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::CaptureNameHasHyphens,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
            return;
        }

        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(name);
            self.error_with_fix(
                DiagnosticKind::CaptureNameUppercase,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
        }
    }

    /// Validate definition name follows PascalCase convention.
    fn validate_def_name(&mut self, name: &str, span: TextRange) {
        if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameLowercase,
                span,
                "definitions map to types",
                format!("use `{}`", suggested),
                suggested,
            );
            return;
        }

        if name.contains('_') || name.contains('-') || name.contains('.') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameHasSeparators,
                span,
                "definitions map to types",
                format!("use `{}`", suggested),
                suggested,
            );
        }
    }

    /// Validate branch label follows PascalCase convention.
    fn validate_branch_label(&mut self, name: &str, span: TextRange) {
        if name.contains('_') || name.contains('-') || name.contains('.') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::BranchLabelHasSeparators,
                span,
                "branch labels map to enum variants",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
        }
    }

    /// Validate field name follows snake_case convention.
    fn validate_field_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') {
            let suggested = name.replace(['.', '-'], "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::FieldNameHasDots,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
            return;
        }

        if name.contains('-') {
            let suggested = name.replace('-', "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::FieldNameHasHyphens,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
            return;
        }

        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(name);
            self.error_with_fix(
                DiagnosticKind::FieldNameUppercase,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
        }
    }

    /// Validate type annotation name (PascalCase for user types, snake_case for primitives allowed).
    fn validate_type_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') || name.contains('-') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::TypeNameInvalidChars,
                span,
                "type annotations map to types",
                format!("use `::{}`", suggested),
                format!("::{}", suggested),
            );
        }
    }
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 && !result.ends_with('_') {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' || c == '.' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

fn capitalize_first(s: &str) -> String {
    assert!(!s.is_empty(), "capitalize_first: called with empty string");
    let mut chars = s.chars();
    let c = chars.next().unwrap();
    c.to_uppercase().chain(chars).collect()
}
