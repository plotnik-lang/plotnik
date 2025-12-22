//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.
//!
//! Replaces the previous `expr_arity.rs` with a more comprehensive type system.

mod context;
mod emit_ts;
mod infer;
mod types;
mod unify;

pub use context::TypeContext;
pub use emit_ts::{EmitConfig, TsEmitter, emit_typescript, emit_typescript_with_config};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeKind,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

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
) -> TypeContext {
    let mut ctx = TypeContext::new();

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
            infer_root(&mut ctx, symbol_table, diag, source_id, root);

            // Register the definition's output type
            if let Some(body) = symbol_table.get(def_name) {
                if let Some(info) = ctx.get_term_info(body).cloned() {
                    let type_id = flow_to_type_id(&mut ctx, &info.flow);
                    ctx.set_def_type(def_name.to_string(), type_id);
                }
            }
        }
    }

    // Handle any definitions not in an SCC (shouldn't happen, but be safe)
    for (name, source_id, _body) in symbol_table.iter_full() {
        if ctx.get_def_type(name).is_some() {
            continue;
        }

        let Some(root) = ast_map.get(&source_id) else {
            continue;
        };

        infer_root(&mut ctx, symbol_table, diag, source_id, root);

        if let Some(body) = symbol_table.get(name) {
            if let Some(info) = ctx.get_term_info(body).cloned() {
                let type_id = flow_to_type_id(&mut ctx, &info.flow);
                ctx.set_def_type(name.to_string(), type_id);
            }
        }
    }

    ctx
}

/// Convert a TypeFlow to a TypeId for storage.
fn flow_to_type_id(ctx: &mut TypeContext, flow: &TypeFlow) -> TypeId {
    match flow {
        TypeFlow::Void => ctx.intern_type(TypeKind::Struct(std::collections::BTreeMap::new())),
        TypeFlow::Scalar(type_id) => *type_id,
        TypeFlow::Fields(fields) => ctx.intern_type(TypeKind::Struct(fields.clone())),
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
