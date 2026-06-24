use super::unify::{UnifyError, unify_flow};
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{PatternFlow, TYPE_NODE};

#[test]
fn unify_scalar_error() {
    let mut ctx = TypeAnalysisBuilder::new();
    let result = unify_flow(&mut ctx, PatternFlow::Value(TYPE_NODE), PatternFlow::Void);
    assert!(matches!(result, Err(UnifyError::ScalarInUnion)));
}
