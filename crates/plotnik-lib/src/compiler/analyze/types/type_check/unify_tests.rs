use super::analysis::TypeAnalysisBuilder;
use super::types::{OutputFlow, TYPE_NODE};
use super::unify::{UnifyError, unify_flow};

#[test]
fn unify_scalar_error() {
    let mut ctx = TypeAnalysisBuilder::new();
    let result = unify_flow(&mut ctx, OutputFlow::Value(TYPE_NODE), OutputFlow::Void);
    assert!(matches!(result, Err(UnifyError::ScalarInUnion)));
}
