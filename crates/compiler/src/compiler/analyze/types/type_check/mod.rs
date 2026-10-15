mod infer;
mod unify;

pub use crate::compiler::analyze::types::type_analysis::TypeAnalysis;
pub use crate::core::Interner;
pub(crate) use infer::{body_output, definition_value_root};

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::analyze::types::naming::{RawTypeNameValidator, TypeNamer};
use crate::compiler::diagnostics::report::Diagnostics;

pub fn infer_types(
    interner: &mut Interner,
    definitions: &DefinitionGraph,
    pattern_facts: &PatternFacts,
    expression: Option<&crate::compiler::analyze::Located<crate::compiler::parse::ast::Pattern>>,
    diag: &mut Diagnostics,
) -> TypeAnalysis {
    let pass = infer::InferPassEnv {
        interner,
        definitions,
        pattern_facts,
        diag,
    };
    let mut types = infer::InferPass::new(pass).run(expression);

    if types.has_built_in_capture_types() {
        RawTypeNameValidator::new(&mut types, interner)
            .validate(definitions, expression.map(|body| body.source()));
    }
    types.normalize_capture_types(interner, diag);
    TypeNamer::new(&mut types, interner, diag)
        .assign(definitions, expression.map(|body| body.source()));
    types.finish()
}
