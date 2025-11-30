//! Grammar productions for the query language.
//!
//! This module implements all `parse_*` methods as an extension of `Parser`.
//! The grammar follows tree-sitter query syntax with extensions for named subqueries.

use rowan::Checkpoint;

use super::core::Parser;
use crate::ql::syntax_kind::token_sets::{
    ALTERNATION_RECOVERY, NAMED_NODE_RECOVERY, PATTERN_FIRST, QUANTIFIERS,
};
use crate::ql::syntax_kind::SyntaxKind;

impl Parser<'_> {
    pub fn parse_root(&mut self) {
        self.start_node(SyntaxKind::Root);

        while self.peek() != SyntaxKind::Error || !self.eof() {
            if self.eof() {
                break;
            }
            self.parse_pattern_or_error();
        }

        self.eat_trivia();
        self.finish_node();
    }

    fn parse_pattern_or_error(&mut self) {
        let kind = self.peek();
        if PATTERN_FIRST.contains(kind) {
            self.parse_pattern();
        } else {
            self.error_and_bump(
                "unexpected token; expected a pattern like (node), [choice], \"literal\", @capture, or _",
            );
        }
    }

    /// Core recursive descent. Dispatches based on lookahead, then checks for quantifier suffix.
    fn parse_pattern(&mut self) {
        if !self.enter_recursion() {
            // On limit: consume everything as error, prevent infinite recursion
            self.start_node(SyntaxKind::Error);
            while !self.eof() {
                self.bump();
            }
            self.finish_node();
            return;
        }

        // Checkpoint before the pattern for potential quantifier wrapping
        let checkpoint = self.checkpoint();

        match self.peek() {
            SyntaxKind::ParenOpen => self.parse_named_node(),
            SyntaxKind::BracketOpen => self.parse_alternation(),
            SyntaxKind::Underscore => self.parse_wildcard(),
            SyntaxKind::StringLit => self.parse_anonymous_node(),
            SyntaxKind::At => self.parse_capture(),
            SyntaxKind::Dot => self.parse_anchor(),
            SyntaxKind::Negation => self.parse_negated_field(),
            SyntaxKind::UpperIdent | SyntaxKind::LowerIdent => self.parse_node_or_field(),
            _ => {
                self.error_and_bump("unexpected token; expected a pattern");
            }
        }

        self.try_parse_quantifier(checkpoint);

        self.exit_recursion();
    }

    /// Named node: `(type child1 child2 ...)` or `(_ child1 ...)` for any node.
    fn parse_named_node(&mut self) {
        self.start_node(SyntaxKind::NamedNode);
        self.expect(SyntaxKind::ParenOpen);

        if self.peek() == SyntaxKind::ParenClose {
            self.error("empty node pattern - expected node type or children");
            self.expect(SyntaxKind::ParenClose);
            self.finish_node();
            return;
        }

        // Optional type constraint: `(identifier ...)` or `(_ ...)` for wildcard
        if matches!(
            self.peek(),
            SyntaxKind::LowerIdent | SyntaxKind::UpperIdent | SyntaxKind::Underscore
        ) {
            self.bump();
        }

        self.parse_node_children(SyntaxKind::ParenClose, NAMED_NODE_RECOVERY);

        self.expect(SyntaxKind::ParenClose);
        self.finish_node();
    }

    /// Parse children until `until` token or recovery set hit.
    /// Recovery set lets parent handle mismatched delimiters gracefully.
    fn parse_node_children(&mut self, until: SyntaxKind, recovery: crate::ql::syntax_kind::TokenSet) {
        while !self.eof() {
            let kind = self.peek();
            if kind == until {
                break;
            }
            if PATTERN_FIRST.contains(kind) {
                self.parse_pattern();
            } else if recovery.contains(kind) {
                break;
            } else {
                self.error_and_bump(
                    "unexpected token inside node; expected a child pattern or closing ')'",
                );
            }
        }
    }

    /// Alternation/choice: `[pattern1 pattern2 ...]`
    fn parse_alternation(&mut self) {
        self.start_node(SyntaxKind::Alternation);
        self.expect(SyntaxKind::BracketOpen);

        self.parse_node_children(SyntaxKind::BracketClose, ALTERNATION_RECOVERY);

        self.expect(SyntaxKind::BracketClose);
        self.finish_node();
    }

    fn parse_wildcard(&mut self) {
        self.start_node(SyntaxKind::Wildcard);
        self.expect(SyntaxKind::Underscore);
        self.finish_node();
    }

    /// Anonymous (literal) node: `"if"`, `"+"`, etc.
    fn parse_anonymous_node(&mut self) {
        self.start_node(SyntaxKind::AnonNode);
        self.expect(SyntaxKind::StringLit);
        self.finish_node();
    }

    /// Capture binding: `@name` or `@name.field.subfield`
    fn parse_capture(&mut self) {
        self.start_node(SyntaxKind::Capture);
        self.expect(SyntaxKind::At);
        if self.peek() == SyntaxKind::CaptureName {
            self.bump();
        } else {
            self.error("expected capture name after '@' (e.g., @name, @func.body)");
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
    fn parse_negated_field(&mut self) {
        self.start_node(SyntaxKind::NegatedField);
        self.expect(SyntaxKind::Negation);
        if matches!(self.peek(), SyntaxKind::LowerIdent) {
            self.bump();
        } else {
            self.error("expected field name after '!' (e.g., !value)");
        }
        self.finish_node();
    }

    /// Disambiguate `field: pattern` from bare identifier via LL(2) lookahead.
    fn parse_node_or_field(&mut self) {
        if self.peek_nth(1) == SyntaxKind::Colon {
            self.parse_field();
        } else {
            self.start_node(SyntaxKind::Pattern);
            self.bump();
            self.finish_node();
        }
    }

    /// Field constraint: `field_name: pattern`
    fn parse_field(&mut self) {
        self.start_node(SyntaxKind::Field);

        if matches!(self.peek(), SyntaxKind::LowerIdent | SyntaxKind::UpperIdent) {
            self.bump();
        } else {
            self.error("expected field name before ':'");
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