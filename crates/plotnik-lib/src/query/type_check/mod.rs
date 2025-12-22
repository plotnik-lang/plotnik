//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.
//!
//! Provides arity validation and type inference for TypeScript emission.

mod context;
mod emit_ts;
mod infer;
mod symbol;
mod types;
mod unify;

pub use context::TypeContext;
pub use emit_ts::{EmitConfig, TsEmitter, emit_typescript, emit_typescript_with_config};
pub use symbol::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeKind,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::diagnostics::Diagnostics;
use crate::parser::ast::Root;
use crate::query::dependencies::DependencyAnalysis;
use crate::query::source_map::SourceId;
use crate::query::symbol_table::{SymbolTable, UNNAMED_DEF};

use infer::infer_root;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    ast_map: &IndexMap<SourceId, Root>,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
    interner: &mut Interner,
) -> TypeContext {
    let mut ctx = TypeContext::new();

    // Seed def mappings from DependencyAnalysis (avoids re-registration)
    ctx.seed_defs(
        dependency_analysis.def_names(),
        dependency_analysis.name_to_def(),
    );

    // Mark recursive definitions before inference.
    // A def is recursive if it's in an SCC with >1 member, or it references itself.
    for scc in &dependency_analysis.sccs {
        let is_recursive_scc = if scc.len() > 1 {
            true
        } else if let Some(name) = scc.first()
            && let Some(body) = symbol_table.get(name)
        {
            body_references_self(body, name)
        } else {
            false
        };

        if is_recursive_scc {
            for def_name in scc {
                let sym = interner.intern(def_name);
                if let Some(def_id) = ctx.get_def_id_sym(sym) {
                    ctx.mark_recursive(def_id);
                }
            }
        }
    }

    // Process definitions in SCC order (leaves first)
    for scc in &dependency_analysis.sccs {
        for def_name in scc {
            // Get the source ID for this definition
            let Some(source_id) = symbol_table.source_id(def_name) else {
                continue;
            };

            let Some(root) = ast_map.get(&source_id) else {
                continue;
            };

            // Run inference on this root
            infer_root(&mut ctx, interner, symbol_table, diag, source_id, root);

            // Register the definition's output type
            if let Some(body) = symbol_table.get(def_name)
                && let Some(info) = ctx.get_term_info(body).cloned()
            {
                let type_id = flow_to_type_id(&mut ctx, &info.flow);
                ctx.set_def_type_by_name(interner, def_name, type_id);
            }
        }
    }

    // Handle any definitions not in an SCC (shouldn't happen, but be safe)
    for (name, source_id, _body) in symbol_table.iter_full() {
        if ctx.get_def_type_by_name(interner, name).is_some() {
            continue;
        }

        let Some(root) = ast_map.get(&source_id) else {
            continue;
        };

        infer_root(&mut ctx, interner, symbol_table, diag, source_id, root);

        if let Some(body) = symbol_table.get(name)
            && let Some(info) = ctx.get_term_info(body).cloned()
        {
            let type_id = flow_to_type_id(&mut ctx, &info.flow);
            ctx.set_def_type_by_name(interner, name, type_id);
        }
    }

    ctx
}

/// Check if an expression body contains a reference to the given name.
fn body_references_self(body: &crate::parser::ast::Expr, name: &str) -> bool {
    use crate::parser::ast::Ref;
    for descendant in body.as_cst().descendants() {
        if let Some(r) = Ref::cast(descendant)
            && let Some(name_tok) = r.name()
            && name_tok.text() == name
        {
            return true;
        }
    }
    false
}

/// Convert a TypeFlow to a TypeId for storage.
fn flow_to_type_id(ctx: &mut TypeContext, flow: &TypeFlow) -> TypeId {
    match flow {
        TypeFlow::Void => ctx.intern_struct(BTreeMap::new()),
        TypeFlow::Scalar(type_id) | TypeFlow::Bubble(type_id) => *type_id,
    }
}

/// Get the primary definition name (first non-underscore, or underscore if none).
pub fn primary_def_name(symbol_table: &SymbolTable) -> &str {
    for name in symbol_table.keys() {
        if name != UNNAMED_DEF {
            return name;
        }
    }
    UNNAMED_DEF
}
