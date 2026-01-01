//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod context;
mod infer;
mod symbol;
pub(crate) mod types;
mod unify;

#[cfg(test)]
mod tests;

pub use context::TypeContext;
pub use symbol::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeShape,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use indexmap::IndexMap;

use crate::analyze::dependencies::DependencyAnalysis;
use crate::analyze::symbol_table::{SymbolTable, UNNAMED_DEF};
use crate::diagnostics::Diagnostics;
use crate::parser::ast::Root;
use crate::query::source_map::SourceId;

use infer::infer_root;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    interner: &mut Interner,
    ast_map: &IndexMap<SourceId, Root>,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) -> TypeContext {
    let ctx = TypeContext::new();
    InferencePass {
        ctx,
        interner,
        ast_map,
        symbol_table,
        dependency_analysis,
        diag,
    }
    .run()
}

struct InferencePass<'a> {
    ctx: TypeContext,
    interner: &'a mut Interner,
    ast_map: &'a IndexMap<SourceId, Root>,
    symbol_table: &'a SymbolTable,
    dependency_analysis: &'a DependencyAnalysis,
    diag: &'a mut Diagnostics,
}

impl<'a> InferencePass<'a> {
    fn run(mut self) -> TypeContext {
        // Avoid re-registration of definitions
        self.ctx.seed_defs(
            self.dependency_analysis.def_names(),
            self.dependency_analysis.name_to_def(),
        );

        self.mark_recursion();
        self.process_sccs();
        self.process_orphans();

        self.ctx
    }

    /// Identify and mark recursive definitions.
    fn mark_recursion(&mut self) {
        for scc in &self.dependency_analysis.sccs {
            for def_name in scc {
                if !self.dependency_analysis.is_recursive(def_name) {
                    continue;
                }
                let sym = self.interner.intern(def_name);
                let Some(def_id) = self.ctx.get_def_id_sym(sym) else {
                    continue;
                };
                self.ctx.mark_recursive(def_id);
            }
        }
    }

    /// Process definitions in SCC order (leaves first).
    fn process_sccs(&mut self) {
        for scc in &self.dependency_analysis.sccs {
            for def_name in scc {
                if let Some(source_id) = self.symbol_table.source_id(def_name) {
                    self.infer_and_register(def_name, source_id);
                }
            }
        }
    }

    /// Handle any definitions not in an SCC (safety net).
    fn process_orphans(&mut self) {
        for (name, source_id, _body) in self.symbol_table.iter_full() {
            // Skip if already processed
            if self.ctx.get_def_type_by_name(self.interner, name).is_some() {
                continue;
            }
            self.infer_and_register(name, source_id);
        }
    }

    fn infer_and_register(&mut self, def_name: &str, source_id: SourceId) {
        let Some(root) = self.ast_map.get(&source_id) else {
            return;
        };

        infer_root(
            &mut self.ctx,
            self.interner,
            self.symbol_table,
            source_id,
            root,
            self.diag,
        );

        // Register the definition's output type based on the inferred body flow
        if let Some(body) = self.symbol_table.get(def_name)
            && let Some(info) = self.ctx.get_term_info(body).cloned()
        {
            let type_id = self.flow_to_type_id(&info.flow);
            self.ctx
                .set_def_type_by_name(self.interner, def_name, type_id);
        }
    }

    fn flow_to_type_id(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => TYPE_VOID,
            TypeFlow::Scalar(id) | TypeFlow::Bubble(id) => *id,
        }
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
