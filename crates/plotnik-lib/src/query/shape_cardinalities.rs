//! Shape cardinality analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ShapeCardinality::One`.
//!
//! Root node cardinality indicates definition count (one vs multiple subqueries),
//! not node matching semantics.

use super::invariants::{
    assert_ref_in_symbols, ensure_ref_body, ensure_ref_name, panic_unexpected_node,
};
use super::named_defs::SymbolTable;
use crate::ast::{Branch, Def, Expr, Field, Ref, Root, Seq, SyntaxNode, Type};
use crate::ast::{Diagnostic, ErrorStage};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeCardinality {
    One,
    Many,
}

pub fn infer(root: &Root, symbols: &SymbolTable) -> HashMap<SyntaxNode, ShapeCardinality> {
    let mut result = HashMap::new();
    let mut def_bodies: HashMap<String, SyntaxNode> = HashMap::new();

    for def in root.defs() {
        if let (Some(name_tok), Some(body)) = (def.name(), def.body()) {
            def_bodies.insert(name_tok.text().to_string(), body.syntax().clone());
        }
    }

    compute_node_cardinality(&root.syntax().clone(), symbols, &def_bodies, &mut result);

    result
}

fn compute_node_cardinality(
    node: &SyntaxNode,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let card = if let Some(&c) = cache.get(node) {
        c
    } else {
        let c = compute_single(node, symbols, def_bodies, cache);
        cache.insert(node.clone(), c);
        c
    };

    for child in node.children() {
        compute_node_cardinality(&child, symbols, def_bodies, cache);
    }

    card
}

fn compute_single(
    node: &SyntaxNode,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let Some(expr) = Expr::cast(node.clone()) else {
        if let Some(root) = Root::cast(node.clone()) {
            let def_count = root.defs().count();
            return if def_count > 1 {
                ShapeCardinality::Many
            } else {
                ShapeCardinality::One
            };
        }
        if let Some(def) = Def::cast(node.clone()) {
            return def
                .body()
                .map(|b| get_or_compute(b.syntax(), symbols, def_bodies, cache))
                .unwrap_or(ShapeCardinality::One);
        }
        if let Some(branch) = Branch::cast(node.clone()) {
            return branch
                .body()
                .map(|b| get_or_compute(b.syntax(), symbols, def_bodies, cache))
                .unwrap_or(ShapeCardinality::One);
        }
        // Type annotations are metadata, not matching expressions
        if Type::cast(node.clone()).is_some() {
            return ShapeCardinality::One;
        }
        panic_unexpected_node(node);
    };

    match expr {
        Expr::Tree(_) => ShapeCardinality::One,
        Expr::Str(_) => ShapeCardinality::One,
        Expr::Wildcard(_) => ShapeCardinality::One,
        Expr::Anchor(_) => ShapeCardinality::One,
        Expr::Field(_) => ShapeCardinality::One,
        Expr::NegatedField(_) => ShapeCardinality::One,
        Expr::Alt(_) => ShapeCardinality::One,

        Expr::Seq(ref seq) => seq_cardinality(seq, symbols, def_bodies, cache),

        Expr::Capture(ref cap) => cap
            .inner()
            .map(|inner| get_or_compute(inner.syntax(), symbols, def_bodies, cache))
            .unwrap_or(ShapeCardinality::One),

        Expr::Quantifier(ref q) => q
            .inner()
            .map(|inner| get_or_compute(inner.syntax(), symbols, def_bodies, cache))
            .unwrap_or(ShapeCardinality::One),

        Expr::Ref(ref r) => ref_cardinality(r, symbols, def_bodies, cache),
    }
}

fn get_or_compute(
    node: &SyntaxNode,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    if let Some(&c) = cache.get(node) {
        return c;
    }
    let c = compute_single(node, symbols, def_bodies, cache);
    cache.insert(node.clone(), c);
    c
}

fn seq_cardinality(
    seq: &Seq,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let children: Vec<_> = seq.children().collect();

    match children.len() {
        0 => ShapeCardinality::One,
        1 => get_or_compute(children[0].syntax(), symbols, def_bodies, cache),
        _ => ShapeCardinality::Many,
    }
}

fn ref_cardinality(
    r: &Ref,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let name_tok = ensure_ref_name(r);
    let name = name_tok.text();

    assert_ref_in_symbols(name, r, symbols.get(name).is_some());

    let body_node = ensure_ref_body(name, r, def_bodies.get(name));

    get_or_compute(body_node, symbols, def_bodies, cache)
}

pub fn validate(
    root: &Root,
    _symbols: &SymbolTable,
    cardinalities: &HashMap<SyntaxNode, ShapeCardinality>,
) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    validate_node(&root.syntax().clone(), cardinalities, &mut errors);
    errors
}

fn validate_node(
    node: &SyntaxNode,
    cardinalities: &HashMap<SyntaxNode, ShapeCardinality>,
    errors: &mut Vec<Diagnostic>,
) {
    if let Some(field) = Field::cast(node.clone())
        && let Some(value) = field.value()
    {
        let card = cardinalities
            .get(value.syntax())
            .copied()
            .unwrap_or(ShapeCardinality::One);

        if card == ShapeCardinality::Many {
            let field_name = field
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "field".to_string());

            errors.push(
                Diagnostic::error(
                    value.syntax().text_range(),
                    format!(
                        "field `{}` value must match a single node, not a sequence",
                        field_name
                    ),
                )
                .with_stage(ErrorStage::Validate),
            );
        }
    }

    for child in node.children() {
        validate_node(&child, cardinalities, errors);
    }
}
