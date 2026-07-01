use super::unify::unify_flow;
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{PatternFlow, TYPE_NODE};

#[test]
fn unify_suppresses_uncaptured_value() {
    // A pending value nothing captures (a bare reference branch) is suppressed,
    // not an error: `[(Foo) (bar)]` is a plain structural alternation.
    let mut ctx = TypeAnalysisBuilder::new();

    let result = unify_flow(&mut ctx, PatternFlow::Value(TYPE_NODE), PatternFlow::Void);

    assert!(matches!(result, Ok(PatternFlow::Void)));
}
