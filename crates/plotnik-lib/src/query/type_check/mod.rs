//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod context;
mod infer;
mod symbol;
mod types;
mod unify;

#[cfg(test)]
mod tests;

pub use context::TypeContext;
pub use symbol::{DefId, Interner, Symbol};
pub use types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeKind,
};
pub use unify::{UnifyError, unify_flow, unify_flows};

use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::diagnostics::Diagnostics;
use crate::parser::ast::{self, Root};
use crate::query::dependencies::DependencyAnalysis;
use crate::query::source_map::SourceId;
use crate::query::symbol_table::{SymbolTable, UNNAMED_DEF};

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
    /// A def is recursive if it's in an SCC with >1 member, or it references itself directly.
    fn mark_recursion(&mut self) {
        for scc in &self.dependency_analysis.sccs {
            if self.is_scc_recursive(scc) {
                for def_name in scc {
                    let sym = self.interner.intern(def_name);
                    if let Some(def_id) = self.ctx.get_def_id_sym(sym) {
                        self.ctx.mark_recursive(def_id);
                    }
                }
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

    fn is_scc_recursive(&self, scc: &[String]) -> bool {
        if scc.len() > 1 {
            return true;
        }

        let Some(name) = scc.first() else {
            return false;
        };

        let Some(body) = self.symbol_table.get(name) else {
            return false;
        };

        body_references_self(body, name)
    }

    fn flow_to_type_id(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => self.ctx.intern_struct(BTreeMap::new()),
            TypeFlow::Scalar(id) | TypeFlow::Bubble(id) => *id,
        }
    }
}

/// Check if an expression body contains a reference to the given name.
fn body_references_self(body: &ast::Expr, name: &str) -> bool {
    body.as_cst().descendants().any(|descendant| {
        let Some(r) = ast::Ref::cast(descendant) else {
            return false;
        };

        let Some(name_tok) = r.name() else {
            return false;
        };

        name_tok.text() == name
    })
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
