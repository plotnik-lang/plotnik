//! Post-SCC re-check of captures on in-progress reference targets.
//!
//! While an SCC is being inferred, a reference to a member that hasn't been
//! registered yet flows as a pending value (`TypeShape::Ref`) — its void-ness
//! is unknown, so the single-referent checks at capture sites stay silent.
//! Once the SCC completes, every member's facts are final; this pass walks
//! exactly those sites again. Sites whose target registered *before* the
//! captor were checked inline with real facts, so the registration-order
//! split keeps every report exactly-once.

use std::collections::HashMap;

use crate::compiler::analyze::types::type_shape::{
    PatternFlow, PatternShape, TYPE_VOID, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::Pattern;

use super::{InferPass, InferState, InferVisitor};

impl InferPass<'_, '_> {
    pub(super) fn check_in_progress_reference_captures(&mut self) {
        let deps = self.analysis.dependency_analysis;

        for scc in deps.sccs() {
            if scc.len() == 1 && !deps.is_recursive_def(scc[0]) {
                continue;
            }
            let registration_order: HashMap<DefId, usize> = scc
                .iter()
                .copied()
                .enumerate()
                .map(|(i, d)| (d, i))
                .collect();

            for (captor_order, &def_id) in scc.iter().enumerate() {
                let name = self
                    .analysis
                    .interner
                    .resolve(deps.def_name_sym(def_id))
                    .to_owned();
                let body = self
                    .analysis
                    .symbol_table
                    .body(&name)
                    .cloned()
                    .expect("symbol-table source entry must have a body");

                let mut visitor = InferVisitor::new(
                    InferState {
                        type_ctx: &mut self.ctx,
                        interner: self.analysis.interner,
                        symbol_table: self.analysis.symbol_table,
                        dependency_analysis: deps,
                        nullable_defs: &self.nullable_defs,
                        diag: &mut *self.analysis.diag,
                    },
                    deps.def_source_id(def_id),
                );
                visitor.recheck_capture_sites(&body, &registration_order, captor_order);
            }
        }
    }
}

impl InferVisitor<'_, '_> {
    fn recheck_capture_sites(
        &mut self,
        pattern: &Pattern,
        registration_order: &HashMap<DefId, usize>,
        captor_order: usize,
    ) {
        let recurse = |visitor: &mut Self, p: &Pattern| {
            visitor.recheck_capture_sites(p, registration_order, captor_order)
        };

        match pattern {
            Pattern::CapturedPattern(cap) => {
                // A suppressed subtree makes no output demands (inline checks
                // skip it too).
                if cap.is_suppressive() {
                    return;
                }
                let Some(inner) = cap.inner() else { return };
                match &inner {
                    Pattern::DefRef(_) => {
                        if let Some(shape) =
                            self.in_progress_target_shape(&inner, registration_order, captor_order)
                        {
                            self.report_capture_on_multi_node_void(&inner, &shape);
                        }
                    }
                    Pattern::QuantifiedPattern(q) => {
                        if let Some(element) = q.inner()
                            && matches!(element, Pattern::DefRef(_))
                            && let Some(shape) = self.in_progress_target_shape(
                                &element,
                                registration_order,
                                captor_order,
                            )
                        {
                            self.report_multi_element_scalar(q, &shape);
                        }
                    }
                    _ => {}
                }
                recurse(self, &inner);
            }
            Pattern::NodePattern(n) => {
                for child in n.children() {
                    recurse(self, &child);
                }
            }
            Pattern::SeqPattern(s) => {
                for child in s.children() {
                    recurse(self, &child);
                }
            }
            Pattern::Union(u) => {
                for branch in u.branches() {
                    if let Some(body) = branch.body() {
                        recurse(self, &body);
                    }
                }
                for p in u.patterns() {
                    recurse(self, &p);
                }
            }
            Pattern::Enum(e) => {
                for branch in e.branches() {
                    if let Some(body) = branch.body() {
                        recurse(self, &body);
                    }
                }
            }
            Pattern::QuantifiedPattern(q) => {
                if let Some(inner) = q.inner() {
                    recurse(self, &inner);
                }
            }
            Pattern::FieldPattern(f) => {
                if let Some(value) = f.value() {
                    recurse(self, &value);
                }
            }
            Pattern::TokenPattern(_) | Pattern::DefRef(_) => {}
        }
    }

    /// The final shape of a same-SCC reference target that was still
    /// in-progress when this site was inferred. `None` for anything the
    /// inline checks already saw with real facts.
    fn in_progress_target_shape(
        &mut self,
        reference: &Pattern,
        registration_order: &HashMap<DefId, usize>,
        captor_order: usize,
    ) -> Option<PatternShape> {
        let Pattern::DefRef(r) = reference else {
            return None;
        };
        let name = r.name()?;
        let def_id = self
            .ctx
            .dependency_analysis
            .def_id_for_name(self.ctx.interner, name.text())?;
        // Outside the SCC, or registered before the captor: the site already
        // saw real facts inline.
        let target_order = registration_order.get(&def_id).copied()?;
        if target_order < captor_order {
            return None;
        }

        let output = self
            .ctx
            .type_ctx
            .in_progress()
            .def_output(def_id)
            .expect("SCC is fully inferred before the re-check");
        let arity = self
            .ctx
            .type_ctx
            .def_arity(def_id)
            .expect("def arities are precomputed before inference");
        let flow = if output == TYPE_VOID {
            PatternFlow::Void
        } else {
            let ref_type = self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id));
            PatternFlow::Value(ref_type)
        };
        Some(PatternShape::new(arity, flow))
    }
}
