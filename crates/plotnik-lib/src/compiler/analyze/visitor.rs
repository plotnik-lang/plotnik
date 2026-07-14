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
//!     fn visit_named_node_pattern(&mut self, node: &Located<NamedNodePattern>) {
//!         // Pre-order logic
//!         walk_named_node_pattern(self, node);
//!         // Post-order logic
//!     }
//! }
//! ```

use crate::compiler::analyze::Located;
use crate::compiler::parse::ast::{
    AlternationPattern, AnonymousNodePattern, CapturedPattern, Def, DefRef, FieldPattern,
    NamedNodePattern, NodeWildcard, Pattern, QuantifiedPattern, Root, SeqPattern,
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

    fn visit_named_node_pattern(&mut self, node: &Located<NamedNodePattern>) {
        walk_named_node_pattern(self, node);
    }

    fn visit_anonymous_node_pattern(&mut self, _node: &Located<AnonymousNodePattern>) {}

    fn visit_node_wildcard(&mut self, _node: &Located<NodeWildcard>) {}

    fn visit_def_ref(&mut self, _ref: &Located<DefRef>) {}

    fn visit_alternation_pattern(&mut self, alternation: &Located<AlternationPattern>) {
        walk_alternation_pattern(self, alternation);
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
        Pattern::NamedNodePattern(n) => visitor.visit_named_node_pattern(&pattern.wrap(n.clone())),
        Pattern::AnonymousNodePattern(n) => {
            visitor.visit_anonymous_node_pattern(&pattern.wrap(n.clone()))
        }
        Pattern::NodeWildcard(n) => visitor.visit_node_wildcard(&pattern.wrap(n.clone())),
        Pattern::DefRef(r) => visitor.visit_def_ref(&pattern.wrap(r.clone())),
        Pattern::Alternation(alternation) => {
            visitor.visit_alternation_pattern(&pattern.wrap(alternation.clone()))
        }
        Pattern::SeqPattern(s) => visitor.visit_seq_pattern(&pattern.wrap(s.clone())),
        Pattern::CapturedPattern(c) => visitor.visit_captured_pattern(&pattern.wrap(c.clone())),
        Pattern::QuantifiedPattern(q) => visitor.visit_quantified_pattern(&pattern.wrap(q.clone())),
        Pattern::FieldPattern(f) => visitor.visit_field_pattern(&pattern.wrap(f.clone())),
    }
}

pub fn walk_named_node_pattern<V: Visitor>(visitor: &mut V, node: &Located<NamedNodePattern>) {
    for child in node.node().children() {
        visitor.visit_pattern(&node.wrap(child));
    }
}

pub fn walk_alternation_pattern<V: Visitor>(
    visitor: &mut V,
    alternation: &Located<AlternationPattern>,
) {
    for pattern in alternation.node().patterns() {
        visitor.visit_pattern(&alternation.wrap(pattern));
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
