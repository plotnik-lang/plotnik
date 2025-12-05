//! Shape cardinality analysis for query expressions.
//!
//! Determines whether an expression matches a single node position (`One`)
//! or multiple sequential positions (`Many`). Used to validate field constraints:
//! `field: expr` requires `expr` to have `ShapeCardinality::One`.
//!
//! `Invalid` marks nodes where cardinality cannot be determined (error nodes,
//! undefined refs, etc.).

use super::Query;
use super::invariants::{
    ensure_capture_has_inner, ensure_quantifier_has_inner, ensure_ref_has_name,
};
use super::symbol_table::SymbolTable;
use crate::diagnostics::Diagnostics;
use crate::parser::{Expr, FieldExpr, Ref, SeqExpr, SyntaxNode, ast};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeCardinality {
    One,
    Many,
    Invalid,
}

impl Query<'_> {
    pub(super) fn infer_shapes(&mut self) {
        let mut def_bodies: HashMap<String, ast::Expr> = HashMap::new();

        for def in self.ast.defs() {
            if let (Some(name_tok), Some(body)) = (def.name(), def.body()) {
                def_bodies.insert(name_tok.text().to_string(), body);
            }
        }

        compute_all_cardinalities(
            self.ast.as_cst(),
            &self.symbol_table,
            &def_bodies,
            &mut self.shape_cardinality_table,
        );
        validate_node(
            self.ast.as_cst(),
            &self.shape_cardinality_table,
            &mut self.shapes_diagnostics,
        );
    }
}

fn compute_all_cardinalities(
    node: &SyntaxNode,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, ast::Expr>,
    cache: &mut HashMap<ast::Expr, ShapeCardinality>,
) {
    if let Some(expr) = Expr::cast(node.clone()) {
        get_or_compute(&expr, symbols, def_bodies, cache);
    }

    for child in node.children() {
        compute_all_cardinalities(&child, symbols, def_bodies, cache);
    }
}

fn compute_single(
    expr: &Expr,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, ast::Expr>,
    cache: &mut HashMap<ast::Expr, ShapeCardinality>,
) -> ShapeCardinality {
    match expr {
        Expr::NamedNode(_) | Expr::AnonymousNode(_) | Expr::FieldExpr(_) | Expr::AltExpr(_) => {
            ShapeCardinality::One
        }

        Expr::SeqExpr(seq) => seq_cardinality(seq, symbols, def_bodies, cache),

        Expr::CapturedExpr(cap) => {
            let inner = ensure_capture_has_inner(cap.inner());
            get_or_compute(&inner, symbols, def_bodies, cache)
        }

        Expr::QuantifiedExpr(q) => {
            let inner = ensure_quantifier_has_inner(q.inner());
            get_or_compute(&inner, symbols, def_bodies, cache)
        }

        Expr::Ref(r) => ref_cardinality(r, symbols, def_bodies, cache),
    }
}

fn get_or_compute(
    expr: &Expr,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, ast::Expr>,
    cache: &mut HashMap<ast::Expr, ShapeCardinality>,
) -> ShapeCardinality {
    if let Some(&c) = cache.get(expr) {
        return c;
    }
    let c = compute_single(expr, symbols, def_bodies, cache);
    cache.insert(expr.clone(), c);
    c
}

fn seq_cardinality(
    seq: &SeqExpr,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, ast::Expr>,
    cache: &mut HashMap<ast::Expr, ShapeCardinality>,
) -> ShapeCardinality {
    let children: Vec<_> = seq.children().collect();

    match children.len() {
        0 => ShapeCardinality::One,
        1 => get_or_compute(&children[0], symbols, def_bodies, cache),
        _ => ShapeCardinality::Many,
    }
}

fn ref_cardinality(
    r: &Ref,
    symbols: &SymbolTable,
    def_bodies: &HashMap<String, ast::Expr>,
    cache: &mut HashMap<ast::Expr, ShapeCardinality>,
) -> ShapeCardinality {
    let name_tok = ensure_ref_has_name(r.name());
    let name = name_tok.text();

    if symbols.get(name).is_none() {
        return ShapeCardinality::Invalid;
    }

    let Some(body) = def_bodies.get(name) else {
        return ShapeCardinality::Invalid;
    };

    get_or_compute(body, symbols, def_bodies, cache)
}

fn validate_node(
    node: &SyntaxNode,
    cardinalities: &HashMap<ast::Expr, ShapeCardinality>,
    errors: &mut Diagnostics,
) {
    if let Some(field) = FieldExpr::cast(node.clone())
        && let Some(value) = field.value()
    {
        let card = cardinalities
            .get(&value)
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
