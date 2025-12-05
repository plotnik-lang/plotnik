//! Shape cardinality analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ShapeCardinality::One`.
//!
//! `Invalid` marks nodes where cardinality cannot be determined (error nodes,
//! undefined refs, etc.).
//!
//! Root node cardinality indicates definition count (one vs multiple subqueries),
//! not node matching semantics.

use super::invariants::{
    ensure_capture_has_inner, ensure_quantifier_has_inner, ensure_ref_has_name,
};
use super::symbol_table::SymbolTable;
use crate::PassResult;
use crate::diagnostics::Diagnostics;
use crate::parser::{Branch, Def, Expr, FieldExpr, Ref, Root, SeqExpr, SyntaxNode, Type};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeCardinality {
    One,
    Many,
    Invalid,
}

pub fn analyze(
    root: &Root,
    symbols: &SymbolTable,
) -> PassResult<HashMap<SyntaxNode, ShapeCardinality>> {
    let mut result = HashMap::new();
    let mut errors = Diagnostics::new();
    let mut def_bodies: HashMap<String, SyntaxNode> = HashMap::new();

    for def in root.defs() {
        if let (Some(name_tok), Some(body)) = (def.name(), def.body()) {
            def_bodies.insert(name_tok.text().to_string(), body.as_cst().clone());
        }
    }

    compute_node_cardinality(&root.as_cst().clone(), symbols, &def_bodies, &mut result);
    validate_node(&root.as_cst().clone(), &result, &mut errors);

    Ok((result, errors))
}

fn compute_node_cardinality(
    node: &SyntaxNode,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let card = get_or_compute(node, symbols, def_bodies, cache);

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
    if let Some(root) = Root::cast(node.clone()) {
        return if root.defs().count() > 1 {
            ShapeCardinality::Many
        } else {
            ShapeCardinality::One
        };
    }

    if let Some(def) = Def::cast(node.clone()) {
        return def
            .body()
            .map(|b| get_or_compute(b.as_cst(), symbols, def_bodies, cache))
            .unwrap_or(ShapeCardinality::Invalid);
    }

    if let Some(branch) = Branch::cast(node.clone()) {
        return branch
            .body()
            .map(|b| get_or_compute(b.as_cst(), symbols, def_bodies, cache))
            .unwrap_or(ShapeCardinality::Invalid);
    }

    // Type annotations are metadata, not matching expressions
    if Type::cast(node.clone()).is_some() {
        return ShapeCardinality::One;
    }

    // Error nodes and other non-Expr nodes: mark as Invalid
    let Some(expr) = Expr::cast(node.clone()) else {
        return ShapeCardinality::Invalid;
    };

    match expr {
        Expr::NamedNode(_) | Expr::AnonymousNode(_) | Expr::FieldExpr(_) | Expr::AltExpr(_) => {
            ShapeCardinality::One
        }

        Expr::SeqExpr(ref seq) => seq_cardinality(seq, symbols, def_bodies, cache),

        Expr::CapturedExpr(ref cap) => {
            let inner = ensure_capture_has_inner(cap.inner());
            get_or_compute(inner.as_cst(), symbols, def_bodies, cache)
        }

        Expr::QuantifiedExpr(ref q) => {
            let inner = ensure_quantifier_has_inner(q.inner());
            get_or_compute(inner.as_cst(), symbols, def_bodies, cache)
        }

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
    seq: &SeqExpr,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let children: Vec<_> = seq.children().collect();

    match children.len() {
        0 => ShapeCardinality::One,
        1 => get_or_compute(children[0].as_cst(), symbols, def_bodies, cache),
        _ => ShapeCardinality::Many,
    }
}

fn ref_cardinality(
    r: &Ref,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, SyntaxNode>,
    cache: &mut HashMap<SyntaxNode, ShapeCardinality>,
) -> ShapeCardinality {
    let name_tok = ensure_ref_has_name(r.name());
    let name = name_tok.text();

    if symbols.get(name).is_none() {
        return ShapeCardinality::Invalid;
    }

    let Some(body_node) = def_bodies.get(name) else {
        return ShapeCardinality::Invalid;
    };

    get_or_compute(body_node, symbols, def_bodies, cache)
}

fn validate_node(
    node: &SyntaxNode,
    cardinalities: &HashMap<SyntaxNode, ShapeCardinality>,
    errors: &mut Diagnostics,
) {
    if let Some(field) = FieldExpr::cast(node.clone())
        && let Some(value) = field.value()
    {
        let card = cardinalities
            .get(value.as_cst())
            .copied()
            .unwrap_or(ShapeCardinality::One);

        if card == ShapeCardinality::Many {
            let field_name = field
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "field".to_string());

            errors
                .error(
                    format!(
                        "field `{}` value must match a single node, not a sequence",
                        field_name
                    ),
                    value.text_range(),
                )
                .emit();
        }
    }

    for child in node.children() {
        validate_node(&child, cardinalities, errors);
    }
}
