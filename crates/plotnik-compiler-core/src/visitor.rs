//! AST Visitor pattern.
//!
//! # Usage
//!
//! Implement `Visitor` for your struct. Override `visit_*` methods to add logic.
//! Call `walk_*` within your override to continue recursion (or omit it to stop).
//!
//! Every node is visited as a [`Located`] so a pass always knows which source the
//! node lives in — diagnostics carry their source by construction.
//!
//! ```ignore
//! impl Visitor for MyPass {
//!     fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
//!         // Pre-order logic
//!         walk_node_pattern(self, node);
//!         // Post-order logic
//!     }
//! }
//! ```

use crate::Located;
use crate::ast::{
    CapturedPattern, Def, EnumPattern, FieldPattern, NodePattern, Pattern, QuantifiedPattern, Ref,
    Root, SeqPattern, TokenPattern, UnionPattern,
};

pub trait Visitor: Sized {
    fn visit(&mut self, ast: &Located<Root>) {
        walk(self, ast);
    }

    fn visit_def(&mut self, def: &Located<Def>) {
        walk_def(self, def);
    }

    fn visit_pattern(&mut self, pattern: &Located<Pattern>) {
        walk_pattern(self, pattern);
    }

    fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
        walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, _node: &Located<TokenPattern>) {}

    fn visit_ref(&mut self, _ref: &Located<Ref>) {}

    fn visit_union_pattern(&mut self, union: &Located<UnionPattern>) {
        walk_union_pattern(self, union);
    }

    fn visit_enum_pattern(&mut self, e: &Located<EnumPattern>) {
        walk_enum_pattern(self, e);
    }

    fn visit_seq_pattern(&mut self, seq: &Located<SeqPattern>) {
        walk_seq_pattern(self, seq);
    }

    fn visit_captured_pattern(&mut self, cap: &Located<CapturedPattern>) {
        walk_captured_pattern(self, cap);
    }

    fn visit_quantified_pattern(&mut self, quant: &Located<QuantifiedPattern>) {
        walk_quantified_pattern(self, quant);
    }

    fn visit_field_pattern(&mut self, field: &Located<FieldPattern>) {
        walk_field_pattern(self, field);
    }
}

pub fn walk<V: Visitor>(visitor: &mut V, ast: &Located<Root>) {
    for def in ast.node().defs() {
        visitor.visit_def(&ast.wrap(def));
    }
}

pub fn walk_def<V: Visitor>(visitor: &mut V, def: &Located<Def>) {
    if let Some(body) = def.node().body() {
        visitor.visit_pattern(&def.wrap(body));
    }
}

pub fn walk_pattern<V: Visitor>(visitor: &mut V, pattern: &Located<Pattern>) {
    match pattern.node() {
        Pattern::NodePattern(n) => visitor.visit_node_pattern(&pattern.wrap(n.clone())),
        Pattern::TokenPattern(n) => visitor.visit_token_pattern(&pattern.wrap(n.clone())),
        Pattern::Ref(r) => visitor.visit_ref(&pattern.wrap(r.clone())),
        Pattern::Union(u) => visitor.visit_union_pattern(&pattern.wrap(u.clone())),
        Pattern::Enum(e) => visitor.visit_enum_pattern(&pattern.wrap(e.clone())),
        Pattern::SeqPattern(s) => visitor.visit_seq_pattern(&pattern.wrap(s.clone())),
        Pattern::CapturedPattern(c) => visitor.visit_captured_pattern(&pattern.wrap(c.clone())),
        Pattern::QuantifiedPattern(q) => visitor.visit_quantified_pattern(&pattern.wrap(q.clone())),
        Pattern::FieldPattern(f) => visitor.visit_field_pattern(&pattern.wrap(f.clone())),
    }
}

pub fn walk_node_pattern<V: Visitor>(visitor: &mut V, node: &Located<NodePattern>) {
    for child in node.node().children() {
        visitor.visit_pattern(&node.wrap(child));
    }
}

pub fn walk_union_pattern<V: Visitor>(visitor: &mut V, union: &Located<UnionPattern>) {
    for branch in union.node().branches() {
        if let Some(body) = branch.body() {
            visitor.visit_pattern(&union.wrap(body));
        }
    }

    for pattern in union.node().patterns() {
        visitor.visit_pattern(&union.wrap(pattern));
    }
}

pub fn walk_enum_pattern<V: Visitor>(visitor: &mut V, e: &Located<EnumPattern>) {
    for branch in e.node().branches() {
        if let Some(body) = branch.body() {
            visitor.visit_pattern(&e.wrap(body));
        }
    }

    for pattern in e.node().patterns() {
        visitor.visit_pattern(&e.wrap(pattern));
    }
}

pub fn walk_seq_pattern<V: Visitor>(visitor: &mut V, seq: &Located<SeqPattern>) {
    for child in seq.node().children() {
        visitor.visit_pattern(&seq.wrap(child));
    }
}

pub fn walk_captured_pattern<V: Visitor>(visitor: &mut V, cap: &Located<CapturedPattern>) {
    if let Some(inner) = cap.node().inner() {
        visitor.visit_pattern(&cap.wrap(inner));
    }
}

pub fn walk_quantified_pattern<V: Visitor>(visitor: &mut V, quant: &Located<QuantifiedPattern>) {
    if let Some(inner) = quant.node().inner() {
        visitor.visit_pattern(&quant.wrap(inner));
    }
}

pub fn walk_field_pattern<V: Visitor>(visitor: &mut V, field: &Located<FieldPattern>) {
    if let Some(val) = field.node().value() {
        visitor.visit_pattern(&field.wrap(val));
    }
}
