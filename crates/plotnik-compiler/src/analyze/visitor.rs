//! AST Visitor pattern.
//!
//! # Usage
//!
//! Implement `Visitor` for your struct. Override `visit_*` methods to add logic.
//! Call `walk_*` within your override to continue recursion (or omit it to stop).
//!
//! ```ignore
//! impl Visitor for MyPass {
//!     fn visit_node_pattern(&mut self, node: &NodePattern) {
//!         // Pre-order logic
//!         walk_node_pattern(self, node);
//!         // Post-order logic
//!     }
//! }
//! ```

use crate::parser::{
    CapturedPattern, Def, EnumPattern, FieldPattern, NodePattern, Pattern, QuantifiedPattern, Ref,
    Root, SeqPattern, TokenPattern, UnionPattern,
};

pub trait Visitor: Sized {
    fn visit(&mut self, ast: &Root) {
        walk(self, ast);
    }

    fn visit_def(&mut self, def: &Def) {
        walk_def(self, def);
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        walk_pattern(self, pattern);
    }

    fn visit_node_pattern(&mut self, node: &NodePattern) {
        walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, _node: &TokenPattern) {}

    fn visit_ref(&mut self, _ref: &Ref) {}

    fn visit_union_pattern(&mut self, union: &UnionPattern) {
        walk_union_pattern(self, union);
    }

    fn visit_enum_pattern(&mut self, e: &EnumPattern) {
        walk_enum_pattern(self, e);
    }

    fn visit_seq_pattern(&mut self, seq: &SeqPattern) {
        walk_seq_pattern(self, seq);
    }

    fn visit_captured_pattern(&mut self, cap: &CapturedPattern) {
        walk_captured_pattern(self, cap);
    }

    fn visit_quantified_pattern(&mut self, quant: &QuantifiedPattern) {
        walk_quantified_pattern(self, quant);
    }

    fn visit_field_pattern(&mut self, field: &FieldPattern) {
        walk_field_pattern(self, field);
    }
}

pub fn walk<V: Visitor>(visitor: &mut V, ast: &Root) {
    for def in ast.defs() {
        visitor.visit_def(&def);
    }
}

pub fn walk_def<V: Visitor>(visitor: &mut V, def: &Def) {
    if let Some(body) = def.body() {
        visitor.visit_pattern(&body);
    }
}

pub fn walk_pattern<V: Visitor>(visitor: &mut V, pattern: &Pattern) {
    match pattern {
        Pattern::NodePattern(n) => visitor.visit_node_pattern(n),
        Pattern::TokenPattern(n) => visitor.visit_token_pattern(n),
        Pattern::Ref(r) => visitor.visit_ref(r),
        Pattern::Union(u) => visitor.visit_union_pattern(u),
        Pattern::Enum(e) => visitor.visit_enum_pattern(e),
        Pattern::SeqPattern(s) => visitor.visit_seq_pattern(s),
        Pattern::CapturedPattern(c) => visitor.visit_captured_pattern(c),
        Pattern::QuantifiedPattern(q) => visitor.visit_quantified_pattern(q),
        Pattern::FieldPattern(f) => visitor.visit_field_pattern(f),
    }
}

pub fn walk_node_pattern<V: Visitor>(visitor: &mut V, node: &NodePattern) {
    for child in node.children() {
        visitor.visit_pattern(&child);
    }
}

pub fn walk_union_pattern<V: Visitor>(visitor: &mut V, union: &UnionPattern) {
    for branch in union.branches() {
        if let Some(body) = branch.body() {
            visitor.visit_pattern(&body);
        }
    }

    for pattern in union.patterns() {
        visitor.visit_pattern(&pattern);
    }
}

pub fn walk_enum_pattern<V: Visitor>(visitor: &mut V, e: &EnumPattern) {
    for branch in e.branches() {
        if let Some(body) = branch.body() {
            visitor.visit_pattern(&body);
        }
    }

    for pattern in e.patterns() {
        visitor.visit_pattern(&pattern);
    }
}

pub fn walk_seq_pattern<V: Visitor>(visitor: &mut V, seq: &SeqPattern) {
    for child in seq.children() {
        visitor.visit_pattern(&child);
    }
}

pub fn walk_captured_pattern<V: Visitor>(visitor: &mut V, cap: &CapturedPattern) {
    if let Some(inner) = cap.inner() {
        visitor.visit_pattern(&inner);
    }
}

pub fn walk_quantified_pattern<V: Visitor>(visitor: &mut V, quant: &QuantifiedPattern) {
    if let Some(inner) = quant.inner() {
        visitor.visit_pattern(&inner);
    }
}

pub fn walk_field_pattern<V: Visitor>(visitor: &mut V, field: &FieldPattern) {
    if let Some(val) = field.value() {
        visitor.visit_pattern(&val);
    }
}
