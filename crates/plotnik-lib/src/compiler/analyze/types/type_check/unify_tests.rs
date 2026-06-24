use super::analysis::TypeAnalysisBuilder;
use super::shapes::{PatternFlow, TYPE_NODE};
use super::unify::{UnifyError, unify_flow};

#[test]
fn unify_scalar_error() {
    let mut ctx = TypeAnalysisBuilder::new();
    let result = unify_flow(&mut ctx, PatternFlow::Value(TYPE_NODE), PatternFlow::Void);
    assert!(matches!(result, Err(UnifyError::ScalarInUnion)));
}
