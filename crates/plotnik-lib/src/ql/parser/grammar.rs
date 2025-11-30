//! Grammar productions for the query language.
//!
//! This module implements all `parse_*` methods as an extension of `Parser`.
//! The grammar follows tree-sitter query syntax with extensions for named subqueries.

use rowan::Checkpoint;

use super::core::Parser;
use crate::ql::syntax_kind::SyntaxKind;
use crate::ql::syntax_kind::token_sets::{
    ALT_RECOVERY, DEF_RECOVERY, NODE_RECOVERY, PATTERN_FIRST, QUANTIFIERS, SEQ_RECOVERY,
};

impl Parser<'_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        while self.peek() != SyntaxKind::Error || !self.eof() {
            if self.eof() {
                break;
            }
            // LL(2): UpperIdent followed by Equals → named definition
            if self.peek() == SyntaxKind::UpperIdent && self.peek_nth(1) == SyntaxKind::Equals {
                self.parse_def();
            } else {
                self.parse_pattern_or_error();
            }
        }

        self.eat_trivia();
        self.finish_node();
    }

    /// Named expression definition: `Name = pattern`
    fn parse_def(&mut self) {
        self.start_node(SyntaxKind::Def);

        // UpperIdent already verified by caller via peek()
        self.bump();

        self.peek(); // skip trivia before '='
        if !self.expect(SyntaxKind::Equals) {
            self.error_recover("expected '=' after name in definition", DEF_RECOVERY);
            self.finish_node();
            return;
        }

        if PATTERN_FIRST.contains(self.peek()) {
            self.parse_pattern();
        } else {
            self.error("expected pattern after '=' in named definition");
        }

        self.finish_node();
    }

    fn parse_pattern_or_error(&mut self) {
        let kind = self.peek();
        if PATTERN_FIRST.contains(kind) {
            self.parse_pattern();
        } else {
            self.error_and_bump(
                "unexpected token; expected a pattern like (node), [choice], {sequence}, \"literal\", @capture, or _",
            );
        }
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier suffix.
    fn parse_pattern(&mut self) {
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
            SyntaxKind::ParenOpen => self.parse_node(),
            SyntaxKind::BracketOpen => self.parse_alt(),
            SyntaxKind::BraceOpen => self.parse_seq(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::StringLit => self.parse_lit(),
            SyntaxKind::At => self.parse_capture(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation => self.parse_negated_field(),
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => self.parse_node_or_field(),
            SyntaxKind::KwError | SyntaxKind::KwMissing => {
                self.error_and_bump(
                    "ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)",
                );
            }
            _ => {
                self.error_and_bump("unexpected token; expected a pattern");
            }
        }

        self.try_parse_quantifier(checkpoint);

        self.exit_recursion();
    }

    /// Node pattern: `(type ...)`, `(_ ...)`, `(ERROR)`, `(MISSING ...)`.
    /// Also handles supertype/subtype: `(expression/binary_expression)`.
    fn parse_node(&mut self) {
        self.start_node(SyntaxKind::Node);
        self.expect(SyntaxKind::ParenOpen);

        match self.peek() {
            SyntaxKind::ParenClose => {
                self.error("empty node pattern - expected node type or children");
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
                    self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY);
                }
                self.expect(SyntaxKind::ParenClose);
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
                        self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY);
                    }
                }
                self.expect(SyntaxKind::ParenClose);
                self.finish_node();
                return;
            }
            _ => {}
        }

        self.parse_children(SyntaxKind::ParenClose, NODE_RECOVERY);
        self.expect(SyntaxKind::ParenClose);
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
                self.error(
                    "unexpected end of input inside node; expected a child pattern or closing delimiter",
                );
                break;
            }
            if PATTERN_FIRST.contains(kind) {
                self.parse_pattern();
            } else if recovery.contains(kind) {
                break;
            } else {
                self.error_and_bump(
                    "unexpected token inside node; expected a child pattern or closing delimiter",
                );
            }
        }
    }

    /// Alternation/choice: `[pattern1 pattern2 ...]` or `[Label: pattern ...]`
    fn parse_alt(&mut self) {
        self.start_node(SyntaxKind::Alt);
        self.expect(SyntaxKind::BracketOpen);

        self.parse_alt_children();

        self.expect(SyntaxKind::BracketClose);
        self.finish_node();
    }

    /// Parse alternation children, handling both tagged `Label: pattern` and unlabeled patterns.
    fn parse_alt_children(&mut self) {
        loop {
            let kind = self.peek();
            if kind == SyntaxKind::BracketClose {
                break;
            }
            if self.eof() {
                self.error(
                    "unexpected end of input inside node; expected a child pattern or closing delimiter",
                );
                break;
            }

            // LL(2): UpperIdent followed by Colon → tagged branch
            if kind == SyntaxKind::UpperIdent && self.peek_nth(1) == SyntaxKind::Colon {
                self.parse_branch();
            } else if PATTERN_FIRST.contains(kind) {
                self.parse_pattern();
            } else if ALT_RECOVERY.contains(kind) {
                break;
            } else {
                self.error_and_bump(
                    "unexpected token inside node; expected a child pattern or closing delimiter",
                );
            }
        }
    }

    /// Tagged alternation branch: `Label: pattern`
    fn parse_branch(&mut self) {
        self.start_node(SyntaxKind::Branch);

        // UpperIdent already verified by caller via peek()
        self.bump();

        self.peek(); // skip trivia before ':'
        self.expect(SyntaxKind::Colon);

        if PATTERN_FIRST.contains(self.peek()) {
            self.parse_pattern();
        } else {
            self.error("expected pattern after label in alternation branch");
        }

        self.finish_node();
    }

    /// Sibling sequence: `{pattern1 pattern2 ...}`
    fn parse_seq(&mut self) {
        self.start_node(SyntaxKind::Seq);
        self.expect(SyntaxKind::BraceOpen);

        self.parse_children(SyntaxKind::BraceClose, SEQ_RECOVERY);

        self.expect(SyntaxKind::BraceClose);
        self.finish_node();
    }

    fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore);
        self.finish_node();
    }

    /// Literal (anonymous) node: `"if"`, `"+"`, etc.
    fn parse_lit(&mut self) {
        self.start_node(SyntaxKind::Lit);
        self.expect(SyntaxKind::StringLit);
        self.finish_node();
    }

    /// Capture binding: `@name` or `@name::Type`
    /// Accepts UpperIdent for resilience; validation will catch casing errors.
    fn parse_capture(&mut self) {
        self.start_node(SyntaxKind::Capture);
        self.expect(SyntaxKind::At);

        match self.peek() {
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent => {
                self.bump();
            }
            _ => {
                self.error("expected capture name after '@' (e.g., @name, @my_var)");
                self.finish_node();
                return;
            }
        }

        if self.peek() == SyntaxKind::DoubleColon {
            self.parse_type_annotation();
        }

        self.finish_node();
    }

    /// Type annotation: `::Type` (UpperIdent) or `::string` (LowerIdent primitive)
    fn parse_type_annotation(&mut self) {
        self.start_node(SyntaxKind::Type);
        self.expect(SyntaxKind::DoubleColon);

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

    /// Anchor for anonymous nodes: `.`
    fn parse_anchor(&mut self) {
        self.start_node(SyntaxKind::Anchor);
        self.expect(SyntaxKind::Dot);
        self.finish_node();
    }

    /// Negated field assertion: `!field` (field must be absent)
    /// Accepts UpperIdent for resilience; validation will catch casing errors.
    fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation);
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

    /// Disambiguate `field: pattern` from bare identifier via LL(2) lookahead.
    fn parse_node_or_field(&mut self) {
        if self.peek_nth(1) == SyntaxKind::Colon {
            self.parse_field();
        } else {
            self.start_node(SyntaxKind::Node);
            self.bump();
            self.finish_node();
        }
    }

    /// Field constraint: `field_name: pattern`
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

        self.expect(SyntaxKind::Colon);

        self.parse_pattern();

        self.finish_node();
    }

    /// If current token is quantifier, wrap preceding pattern using checkpoint.
    fn try_parse_quantifier(&mut self, checkpoint: Checkpoint) {
        if self.at_set(QUANTIFIERS) {
            self.start_node_at(checkpoint, SyntaxKind::Quantifier);
            self.bump();
            self.finish_node();
        }
    }
}
