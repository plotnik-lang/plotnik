//! Grammar productions for the query language.
//!
//! This module implements all `parse_*` methods as an extension of `Parser`.
//! The grammar follows tree-sitter query syntax with extensions for named subqueries.

use rowan::{Checkpoint, TextRange};

use super::core::Parser;
use super::error::{Fix, RelatedInfo};
use crate::ql::lexer::token_text;
use crate::ql::syntax_kind::SyntaxKind;
use crate::ql::syntax_kind::token_sets::{
    ALT_RECOVERY, DEF_RECOVERY, EXPR_FIRST, QUANTIFIERS, SEPARATORS, SEQ_RECOVERY, TREE_RECOVERY,
};

impl Parser<'_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        // Track spans of unnamed defs to emit errors for non-last ones
        let mut unnamed_def_spans: Vec<TextRange> = Vec::new();

        while self.peek() != SyntaxKind::Error || !self.eof() {
            if self.eof() {
                break;
            }
            // LL(2): UpperIdent followed by Equals → named definition
            if self.peek() == SyntaxKind::UpperIdent && self.peek_nth(1) == SyntaxKind::Equals {
                self.parse_def();
            } else {
                // Anonymous def: wrap expression in Def node
                let start = self.current_span().start();
                self.start_node(SyntaxKind::Def);
                self.parse_expr_or_error();
                self.finish_node();
                // Record span for later validation (only last unnamed def is allowed)
                // Find last non-trivia token end (peek() may have buffered trailing trivia)
                let end = self.last_non_trivia_end().unwrap_or(start);
                unnamed_def_spans.push(TextRange::new(start, end));
            }
        }

        // Emit errors for all unnamed defs except the last one
        if unnamed_def_spans.len() > 1 {
            for span in &unnamed_def_spans[..unnamed_def_spans.len() - 1] {
                let def_text = &self.source[usize::from(span.start())..usize::from(span.end())];
                self.errors.push(super::error::SyntaxError::new(
                    *span,
                    format!(
                        "unnamed definition must be last in file; add a name: `Name = {}`",
                        def_text.trim()
                    ),
                ));
            }
        }

        self.eat_trivia();
        self.finish_node();
    }

    /// Named expression definition: `Name = expr`
    fn parse_def(&mut self) {
        self.start_node(SyntaxKind::Def);

        // UpperIdent already verified by caller via peek()
        self.bump();

        self.peek();
        if !self.expect(SyntaxKind::Equals, "'=' after definition name") {
            self.error_recover("expected '=' after name in definition", DEF_RECOVERY);
            self.finish_node();
            return;
        }

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error("expected expression after '=' in named definition");
        }

        self.finish_node();
    }

    fn parse_expr_or_error(&mut self) {
        let kind = self.peek();
        if EXPR_FIRST.contains(kind) {
            self.parse_expr();
        } else if kind == SyntaxKind::At {
            self.error_and_bump("capture '@' must follow an expression to capture");
        } else if kind == SyntaxKind::Predicate {
            self.error_and_bump(
                "tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported",
            );
        } else {
            self.error_and_bump(
                "unexpected token; expected an expression like (node), [choice], {sequence}, \"literal\", or _",
            );
        }
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier/capture suffix.
    fn parse_expr(&mut self) {
        if !self.enter_recursion() {
            self.start_node(SyntaxKind::Error);
            while !self.eof() {
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
            SyntaxKind::StringLit => self.parse_lit(),
            SyntaxKind::SingleQuoteLit => self.parse_single_quote_lit(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation => self.parse_negated_field(),
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => self.parse_tree_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(
                    "ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)",
                );
            }
            _ => {
                self.error_and_bump("unexpected token; expected an expression");
            }
        }

        self.try_parse_quantifier(checkpoint);
        self.try_parse_capture(checkpoint);

        self.exit_recursion();
    }

    /// Tree expression: `(type ...)`, `(_ ...)`, `(ERROR)`, `(MISSING ...)`.
    /// Also handles supertype/subtype: `(expression/binary_expression)`.
    fn parse_tree(&mut self) {
        self.start_node(SyntaxKind::Tree);
        self.push_delimiter(SyntaxKind::ParenOpen);
        self.expect(SyntaxKind::ParenOpen, "opening '(' for node");

        match self.peek() {
            SyntaxKind::ParenClose => {
                self.error("empty tree expression - expected node type or children");
            }
            SyntaxKind::Underscore => {
                self.bump();
            }
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent => {
                self.bump();
                if self.peek() == SyntaxKind::Slash {
                    self.bump();
                    match self.peek() {
                        SyntaxKind::LowerIdent | SyntaxKind::StringLit => {
                            self.bump();
                        }
                        _ => {
                            self.error(
                                "expected subtype after '/' (e.g., expression/binary_expression)",
                            );
                        }
                    }
                }
            }
            SyntaxKind::KwError => {
                self.bump();
                if self.peek() != SyntaxKind::ParenClose {
                    self.error("(ERROR) takes no arguments");
                    self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
                }
                self.pop_delimiter();
                self.expect(SyntaxKind::ParenClose, "closing ')' for (ERROR)");
                self.finish_node();
                return;
            }
            SyntaxKind::KwMissing => {
                self.bump();
                match self.peek() {
                    SyntaxKind::LowerIdent | SyntaxKind::StringLit => {
                        self.bump();
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
            _ => {}
        }

        self.parse_children(SyntaxKind::ParenClose, TREE_RECOVERY);
        self.pop_delimiter();
        self.expect(SyntaxKind::ParenClose, "closing ')' for tree");
        self.finish_node();
    }

    /// Parse children until `until` token or recovery set hit.
    fn parse_children(&mut self, until: SyntaxKind, recovery: crate::ql::syntax_kind::TokenSet) {
        loop {
            let kind = self.peek();
            if kind == until {
                break;
            }
            if self.eof() {
                let (construct, delim) = match until {
                    SyntaxKind::ParenClose => ("tree", "')'"),
                    SyntaxKind::BraceClose => ("sequence", "'}'"),
                    _ => ("construct", "closing delimiter"),
                };
                let msg = format!("unclosed {construct}; expected {delim}");
                if let Some(open) = self.delimiter_stack.last() {
                    let related = RelatedInfo::new(open.span, format!("{construct} started here"));
                    self.error_with_related(msg, related);
                } else {
                    self.error(msg);
                }
                break;
            }
            if SEPARATORS.contains(kind) {
                self.error_skip_separator();
                continue;
            }
            if EXPR_FIRST.contains(kind) {
                self.parse_expr();
            } else if kind == SyntaxKind::Predicate {
                self.error_and_bump(
                    "tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported",
                );
            } else if recovery.contains(kind) {
                break;
            } else {
                self.error_and_bump(
                    "unexpected token; expected a child expression or closing delimiter",
                );
            }
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
            let kind = self.peek();
            if kind == SyntaxKind::BracketClose {
                break;
            }
            if self.eof() {
                let msg = "unclosed alternation; expected ']'";
                if let Some(open) = self.delimiter_stack.last() {
                    let related = RelatedInfo::new(open.span, "alternation started here");
                    self.error_with_related(msg, related);
                } else {
                    self.error(msg);
                }
                break;
            }
            if SEPARATORS.contains(kind) {
                self.error_skip_separator();
                continue;
            }

            // LL(2): UpperIdent followed by Colon → tagged branch
            if kind == SyntaxKind::UpperIdent && self.peek_nth(1) == SyntaxKind::Colon {
                self.parse_branch();
            // LL(2): LowerIdent followed by Colon → likely mistyped branch label
            } else if kind == SyntaxKind::LowerIdent && self.peek_nth(1) == SyntaxKind::Colon {
                self.parse_branch_lowercase_label();
            } else if EXPR_FIRST.contains(kind) {
                self.parse_expr();
            } else if ALT_RECOVERY.contains(kind) {
                break;
            } else {
                self.error_and_bump(
                    "unexpected token; expected a child expression or closing delimiter",
                );
            }
        }
    }

    /// Tagged alternation branch: `Label: expr`
    fn parse_branch(&mut self) {
        self.start_node(SyntaxKind::Branch);

        // UpperIdent already verified by caller via peek()
        self.bump();

        self.peek();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error("expected expression after branch label");
        }

        self.finish_node();
    }

    /// Parse a branch with lowercase label - parse as Branch but emit error.
    fn parse_branch_lowercase_label(&mut self) {
        self.start_node(SyntaxKind::Branch);

        let span = self.current_span();
        let label_text = token_text(self.source, &self.tokens[self.pos]);
        let capitalized = capitalize_first(label_text);

        let fix = Fix::new(
            capitalized.clone(),
            format!("capitalize as `{}`", capitalized),
        );
        self.error_with_fix(
            span,
            "tagged alternation labels must be Capitalized (they map to enum variants)",
            fix,
        );

        self.bump();
        self.peek();
        self.expect(SyntaxKind::Colon, "':' after branch label");

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error("expected expression after branch label");
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

    /// Literal (anonymous) node: `"if"`, `"+"`, etc.
    fn parse_lit(&mut self) {
        self.start_node(SyntaxKind::Lit);
        self.expect(SyntaxKind::StringLit, "double-quoted string literal");
        self.finish_node();
    }

    /// Single-quoted literal - parse as Lit but emit error about using double quotes.
    fn parse_single_quote_lit(&mut self) {
        self.start_node(SyntaxKind::Lit);

        let span = self.current_span();
        let text = token_text(self.source, &self.tokens[self.pos]);
        // Convert 'foo' to "foo"
        let inner = &text[1..text.len() - 1];
        let fix = Fix::new(format!("\"{}\"", inner), "use double quotes");
        self.error_with_fix(span, "single quotes are not valid for string literals", fix);

        self.bump();
        self.finish_node();
    }

    /// Parse capture suffix: `@name` or `@name :: Type`
    /// Called after the expression to capture has already been parsed.
    /// Accepts UpperIdent for resilience; validation will catch casing errors.
    /// Detects tree-sitter style dotted captures (`@foo.bar.baz`) and emits helpful errors.
    fn parse_capture_suffix(&mut self, at_span_start: rowan::TextSize) {
        self.expect(SyntaxKind::At, "'@' for capture");

        match self.peek() {
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent => {
                self.bump();
            }
            _ => {
                self.error("expected capture name after '@' (e.g., @name, @my_var)");
                return;
            }
        }

        // Detect tree-sitter style dotted captures: @foo.bar.baz
        // Only trigger when tokens are adjacent (no whitespace)
        if self.check_and_consume_dotted_capture(at_span_start) {
            return;
        }

        // Check for single colon (common mistake: @x : Type instead of @x :: Type)
        if self.peek() == SyntaxKind::Colon {
            self.parse_type_annotation_single_colon();
        } else if self.peek() == SyntaxKind::DoubleColon {
            self.parse_type_annotation();
        }
    }

    /// Check for adjacent dotted capture name (`@foo.bar.baz`) and consume it if present.
    /// Returns true if a dotted capture was found (error already emitted).
    fn check_and_consume_dotted_capture(&mut self, at_start: rowan::TextSize) -> bool {
        // Check if current token (without skipping trivia) is an adjacent Dot
        if self.current() != SyntaxKind::Dot || !self.is_adjacent_to_prev() {
            return false;
        }

        // The first part was already consumed, get it from the previous token
        let mut parts: Vec<String> = Vec::new();
        if let Some(prev_span) = self.prev_span() {
            let prev_text = token_text(
                self.source,
                &crate::ql::lexer::Token {
                    kind: SyntaxKind::LowerIdent,
                    span: prev_span,
                },
            );
            parts.push(prev_text.to_string());
        }

        while self.current() == SyntaxKind::Dot && self.is_adjacent_to_prev() {
            self.bump();

            if (self.current() == SyntaxKind::LowerIdent
                || self.current() == SyntaxKind::UpperIdent)
                && self.is_adjacent_to_prev()
            {
                let ident_text = token_text(self.source, &self.tokens[self.pos]);
                parts.push(ident_text.to_string());
                self.bump();
            } else {
                break;
            }
        }

        let end = self.prev_span().map_or(at_start, |s| s.end());
        let error_range = TextRange::new(at_start, end);

        let suggested_name = parts.join("_");
        let fix = Fix::new(
            format!("@{}", suggested_name),
            format!(
                "captures become struct fields; use @{} instead",
                suggested_name
            ),
        );

        self.error_with_fix(error_range, "capture names cannot contain dots", fix);

        true
    }

    /// Type annotation: `::Type` (UpperIdent) or `::string` (LowerIdent primitive)
    fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon, "'::' for type annotation");

        match self.peek() {
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => {
                self.bump();
            }
            _ => {
                self.error("expected type name after '::' (e.g., ::MyType or ::string)");
            }
        }

        self.finish_node();
    }

    /// Handle single colon type annotation (common mistake: `@x : Type` instead of `@x :: Type`)
    fn parse_type_annotation_single_colon(&mut self) {
        // Check if followed by something that looks like a type
        if !matches!(
            self.peek_nth(1),
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent
        ) {
            return;
        }

        self.start_node(SyntaxKind::Type);

        let span = self.current_span();
        let fix = Fix::new("::", "use '::'");
        self.error_with_fix(span, "single colon is not valid for type annotations", fix);

        self.bump();

        match self.peek() {
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => {
                self.bump();
            }
            _ => {}
        }

        self.finish_node();
    }

    /// Anchor for anonymous nodes: `.`
    fn parse_anchor(&mut self) {
        self.start_node(SyntaxKind::Anchor);
        self.expect(SyntaxKind::Dot, "'.' anchor");
        self.finish_node();
    }

    /// Negated field assertion: `!field` (field must be absent)
    /// Accepts UpperIdent for resilience; validation will catch casing errors.
    fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation, "'!' for negated field");
        match self.peek() {
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent => {
                self.bump();
            }
            _ => {
                self.error("expected field name after '!' (e.g., !value)");
            }
        }
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
            self.start_node(SyntaxKind::Tree);
            self.bump();
            self.finish_node();
        }
    }

    /// Field constraint: `field_name: expr`
    /// Accepts UpperIdent for resilience; validation will catch casing errors.
    fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        match self.peek() {
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent => {
                self.bump();
            }
            _ => {
                self.error("expected field name before ':'");
            }
        }

        self.expect(
            SyntaxKind::Colon,
            "':' to separate field name from its value",
        );

        self.parse_expr();

        self.finish_node();
    }

    /// Handle `field = expr` typo - parse as Field but emit error.
    fn parse_field_equals_typo(&mut self) {
        self.start_node(SyntaxKind::Field);

        self.bump();
        self.peek();
        let span = self.current_span();
        let fix = Fix::new(":", "use ':'");
        self.error_with_fix(span, "'=' is not valid for field constraints", fix);
        self.bump();

        if EXPR_FIRST.contains(self.peek()) {
            self.parse_expr();
        } else {
            self.error("expected expression after field name");
        }

        self.finish_node();
    }

    /// Skip a separator token (comma or pipe) and emit helpful error.
    fn error_skip_separator(&mut self) {
        let kind = self.current();
        let span = self.current_span();
        let char_name = match kind {
            SyntaxKind::Comma => ",",
            SyntaxKind::Pipe => "|",
            _ => return,
        };
        let fix = Fix::new("", "remove separator");
        self.error_with_fix(
            span,
            format!(
                "'{}' is not valid syntax; plotnik uses whitespace for separation",
                char_name
            ),
            fix,
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

    /// If current token is `@`, wrap preceding expression with Capture using checkpoint.
    fn try_parse_capture(&mut self, checkpoint: Checkpoint) {
        if self.peek() == SyntaxKind::At {
            let at_span_start = self.current_span().start();
            self.start_node_at(checkpoint, SyntaxKind::Capture);
            self.parse_capture_suffix(at_span_start);
            self.finish_node();
        }
    }
}

/// Capitalize the first letter of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}
