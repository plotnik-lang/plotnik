//! Name resolution: builds symbol table and checks references.
//!
//! Two-pass approach:
//! 1. Collect all `Name = expr` definitions
//! 2. Check that all `(UpperIdent)` references are defined

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use crate::parser::{Diagnostic, ErrorStage};
use crate::parser::{Expr, Ref, Root};

#[derive(Debug, Clone)]
pub struct SymbolTable {
    defs: IndexMap<String, DefInfo>,
}

#[derive(Debug, Clone)]
pub struct DefInfo {
    pub name: String,
    pub range: TextRange,
    pub refs: IndexSet<String>,
}

#[derive(Debug)]
pub struct ResolveResult {
    pub symbols: SymbolTable,
    pub errors: Vec<Diagnostic>,
}

impl SymbolTable {
    pub fn get(&self, name: &str) -> Option<&DefInfo> {
        self.defs.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.defs.keys().map(|s| s.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &DefInfo> {
        self.defs.values()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

pub fn resolve(root: &Root) -> ResolveResult {
    let mut defs = IndexMap::new();
    let mut errors = Vec::new();

    // Pass 1: collect definitions
    for def in root.defs() {
        let Some(name_token) = def.name() else {
            continue;
        };

        let name = name_token.text().to_string();
        let range = name_token.text_range();

        if defs.contains_key(&name) {
            errors.push(
                Diagnostic::error(range, format!("duplicate definition: `{}`", name))
                    .with_stage(ErrorStage::Resolve),
            );
            continue;
        }

        let mut refs = IndexSet::new();
        if let Some(body) = def.body() {
            collect_refs(&body, &mut refs);
        }
        defs.insert(name.clone(), DefInfo { name, range, refs });
    }

    let symbols = SymbolTable { defs };

    // Pass 2: check references
    for def in root.defs() {
        let Some(body) = def.body() else { continue };
        collect_reference_errors(&body, &symbols, &mut errors);
    }

    // Parser wraps all top-level exprs in Def nodes, so this should be empty
    assert!(
        root.exprs().next().is_none(),
        "named_defs: unexpected bare Expr in Root (parser should wrap in Def)"
    );

    ResolveResult { symbols, errors }
}

fn collect_refs(expr: &Expr, refs: &mut IndexSet<String>) {
    match expr {
        Expr::Ref(r) => {
            let Some(name_token) = r.name() else { return };
            refs.insert(name_token.text().to_string());
        }
        Expr::Tree(tree) => {
            for child in tree.children() {
                collect_refs(&child, refs);
            }
        }
        Expr::Alt(alt) => {
            for branch in alt.branches() {
                let Some(body) = branch.body() else { continue };
                collect_refs(&body, refs);
            }
            // Parser wraps all alt children in Branch nodes
            assert!(
                alt.exprs().next().is_none(),
                "named_defs: unexpected bare Expr in Alt (parser should wrap in Branch)"
            );
        }
        Expr::Seq(seq) => {
            for child in seq.children() {
                collect_refs(&child, refs);
            }
        }
        Expr::Capture(cap) => {
            let Some(inner) = cap.inner() else { return };
            collect_refs(&inner, refs);
        }
        Expr::Quantifier(q) => {
            let Some(inner) = q.inner() else { return };
            collect_refs(&inner, refs);
        }
        Expr::Field(f) => {
            let Some(value) = f.value() else { return };
            collect_refs(&value, refs);
        }
        Expr::Str(_) | Expr::Wildcard(_) | Expr::Anchor(_) | Expr::NegatedField(_) => {}
    }
}

fn collect_reference_errors(expr: &Expr, symbols: &SymbolTable, errors: &mut Vec<Diagnostic>) {
    match expr {
        Expr::Ref(r) => {
            check_ref_reference(r, symbols, errors);
        }
        Expr::Tree(tree) => {
            for child in tree.children() {
                collect_reference_errors(&child, symbols, errors);
            }
        }
        Expr::Alt(alt) => {
            for branch in alt.branches() {
                let Some(body) = branch.body() else { continue };
                collect_reference_errors(&body, symbols, errors);
            }
            // Parser wraps all alt children in Branch nodes
            assert!(
                alt.exprs().next().is_none(),
                "named_defs: unexpected bare Expr in Alt (parser should wrap in Branch)"
            );
        }
        Expr::Seq(seq) => {
            for child in seq.children() {
                collect_reference_errors(&child, symbols, errors);
            }
        }
        Expr::Capture(cap) => {
            let Some(inner) = cap.inner() else { return };
            collect_reference_errors(&inner, symbols, errors);
        }
        Expr::Quantifier(q) => {
            let Some(inner) = q.inner() else { return };
            collect_reference_errors(&inner, symbols, errors);
        }
        Expr::Field(f) => {
            let Some(value) = f.value() else { return };
            collect_reference_errors(&value, symbols, errors);
        }
        Expr::Str(_) | Expr::Wildcard(_) | Expr::Anchor(_) | Expr::NegatedField(_) => {}
    }
}

fn check_ref_reference(r: &Ref, symbols: &SymbolTable, errors: &mut Vec<Diagnostic>) {
    let Some(name_token) = r.name() else { return };
    let name = name_token.text();

    if symbols.get(name).is_some() {
        return;
    }

    errors.push(
        Diagnostic::error(
            name_token.text_range(),
            format!("undefined reference: `{}`", name),
        )
        .with_stage(ErrorStage::Resolve),
    );
}
