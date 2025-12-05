//! Symbol table: name resolution and reference checking.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::IndexMap;

use crate::diagnostics::Diagnostics;
use crate::parser::{Expr, Ref, Root, ast};

use super::Query;

pub type SymbolTable<'src> = IndexMap<&'src str, ast::Expr>;

impl<'a> Query<'a> {
    pub(super) fn resolve_names(&mut self) {
        let (symbols, diagnostics) = resolve(&self.ast, self.source);
        self.symbol_table = symbols;
        self.resolve_diagnostics = diagnostics;
    }
}

fn resolve<'src>(root: &Root, source: &'src str) -> (SymbolTable<'src>, Diagnostics) {
    let mut symbols: SymbolTable<'src> = IndexMap::new();
    let mut diagnostics = Diagnostics::new();

    // Pass 1: collect definitions
    for def in root.defs() {
        let Some(name_token) = def.name() else {
            continue;
        };

        let range = name_token.text_range();
        let name = &source[range.start().into()..range.end().into()];

        if symbols.contains_key(name) {
            diagnostics
                .error(format!("duplicate definition: `{}`", name), range)
                .emit();
            continue;
        }

        if let Some(body) = def.body() {
            symbols.insert(name, body);
        }
    }

    // Pass 2: check references
    for def in root.defs() {
        let Some(body) = def.body() else { continue };
        collect_reference_diagnostics(&body, &symbols, &mut diagnostics);
    }

    // Parser wraps all top-level exprs in Def nodes, so this should be empty
    assert!(
        root.exprs().next().is_none(),
        "symbol_table: unexpected bare Expr in Root (parser should wrap in Def)"
    );

    (symbols, diagnostics)
}

fn collect_reference_diagnostics(
    expr: &Expr,
    symbols: &SymbolTable<'_>,
    diagnostics: &mut Diagnostics,
) {
    match expr {
        Expr::Ref(r) => {
            check_ref_diagnostic(r, symbols, diagnostics);
        }
        Expr::NamedNode(node) => {
            for child in node.children() {
                collect_reference_diagnostics(&child, symbols, diagnostics);
            }
        }
        Expr::AltExpr(alt) => {
            for branch in alt.branches() {
                let Some(body) = branch.body() else { continue };
                collect_reference_diagnostics(&body, symbols, diagnostics);
            }
            // Parser wraps all alt children in Branch nodes
            assert!(
                alt.exprs().next().is_none(),
                "symbol_table: unexpected bare Expr in Alt (parser should wrap in Branch)"
            );
        }
        Expr::SeqExpr(seq) => {
            for child in seq.children() {
                collect_reference_diagnostics(&child, symbols, diagnostics);
            }
        }
        Expr::CapturedExpr(cap) => {
            let Some(inner) = cap.inner() else { return };
            collect_reference_diagnostics(&inner, symbols, diagnostics);
        }
        Expr::QuantifiedExpr(q) => {
            let Some(inner) = q.inner() else { return };
            collect_reference_diagnostics(&inner, symbols, diagnostics);
        }
        Expr::FieldExpr(f) => {
            let Some(value) = f.value() else { return };
            collect_reference_diagnostics(&value, symbols, diagnostics);
        }
        Expr::AnonymousNode(_) => {}
    }
}

fn check_ref_diagnostic(r: &Ref, symbols: &SymbolTable<'_>, diagnostics: &mut Diagnostics) {
    let Some(name_token) = r.name() else { return };
    let name = name_token.text();

    if symbols.contains_key(name) {
        return;
    }

    diagnostics
        .error(
            format!("undefined reference: `{}`", name),
            name_token.text_range(),
        )
        .emit();
}
