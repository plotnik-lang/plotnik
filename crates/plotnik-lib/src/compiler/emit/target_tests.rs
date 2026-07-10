use crate::compiler::query::QueryBuilder;
use crate::compiler::test_utils::synthetic_grammar;
use crate::compiler::{
    Error, RustCodegenConfig, TypeScriptCodegenConfig, TypeScriptNodeRepresentation,
};

#[test]
fn invalid_target_configuration_is_a_spanless_query_error() {
    let compiled = QueryBuilder::from_inline("Q = (program)")
        .compile(synthetic_grammar())
        .expect("target-neutral compilation answers");

    let Err(rust_error) = compiled.emit(RustCodegenConfig::new().runtime_crate("::")) else {
        panic!("invalid Rust configuration is rejected");
    };
    assert!(matches!(rust_error, Error::EmitConfig(_)));

    let Err(typescript_error) = compiled.emit_types(
        TypeScriptCodegenConfig::new()
            .node_representation(TypeScriptNodeRepresentation::LiveTreeSitterNode),
    ) else {
        panic!("unsupported TypeScript configuration is rejected");
    };
    assert!(matches!(typescript_error, Error::EmitConfig(_)));
    assert!(compiled.diagnostics().is_empty());
}
