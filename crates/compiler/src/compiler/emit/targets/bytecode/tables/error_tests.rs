use std::fmt::Write as _;

use crate::bytecode::{EncodeError, Module};

use super::EmitError;
use crate::compiler::query::QueryBuilder;
use crate::compiler::test_utils::synthetic_grammar as grammar;
use crate::compiler::{BytecodeConfig, DiagnosticKind, RustCodegenConfig};
use crate::compiler::{SourceMap, SourcePath};

#[track_caller]
fn try_emit(src: &str) -> Result<Vec<u8>, EmitError> {
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("query.ptk"), src);
    let query = QueryBuilder::new(source_map)
        .bind(grammar())
        .expect("query parses");
    assert!(query.is_valid(), "query should bind to the grammar:\n{src}");
    query.emit_bytecode_for_test()
}

#[test]
fn fields_256_are_source_capable_but_exceed_the_bytecode_target() {
    let mut query = String::from("Q = (program");
    for index in 0..=u8::MAX {
        write!(query, " (expression_statement) @field_{index} :: text").unwrap();
    }
    query.push(')');
    let compiled = QueryBuilder::from_inline(&query)
        .compile(grammar())
        .expect("target-neutral compilation answers");

    assert!(compiled.is_valid());
    assert!(
        compiled
            .emit_types(RustCodegenConfig::new())
            .expect("Rust type emission answers")
            .is_valid()
    );
    assert!(
        compiled
            .emit(RustCodegenConfig::new())
            .expect("Rust module emission answers")
            .is_valid()
    );

    let bytecode = compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode target answers with a domain rejection");
    assert!(bytecode.artifact().is_none());
    assert_eq!(
        bytecode.diagnostics().kinds().collect::<Vec<_>>(),
        vec![DiagnosticKind::TargetLimitExceeded]
    );
}

#[test]
fn variant_cases_256_are_source_capable_but_exceed_the_bytecode_target() {
    let mut query = String::from("Q = (program [");
    for index in 0..=u8::MAX {
        write!(query, " Variant{index}: (_) @value_{index}").unwrap();
    }
    query.push_str("] @choice)");
    let compiled = QueryBuilder::from_inline(&query)
        .compile(grammar())
        .expect("target-neutral compilation answers");

    assert!(compiled.is_valid());
    assert!(
        compiled
            .emit_types(RustCodegenConfig::new())
            .expect("Rust type emission answers")
            .is_valid()
    );
    assert!(
        compiled
            .emit(RustCodegenConfig::new())
            .expect("Rust module emission answers")
            .is_valid()
    );

    let bytecode = compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode target answers with a domain rejection");
    assert!(bytecode.artifact().is_none());
    assert_eq!(
        bytecode.diagnostics().kinds().collect::<Vec<_>>(),
        vec![DiagnosticKind::TargetLimitExceeded]
    );
}

#[test]
fn effect_member_payload_overflow_is_emit_error() {
    let mut query = String::new();
    for def in 0..5 {
        write!(query, "D{def} = (program").unwrap();
        for field in 0..250 {
            write!(query, " (expression_statement) @d{def}_f{field}").unwrap();
        }
        writeln!(query, ")").unwrap();
    }

    let err = try_emit(&query).expect_err("> 1023 members must not encode");
    assert!(
        matches!(
            err,
            EmitError::Encode(EncodeError::EffectPayloadOverflow(_))
        ),
        "got {err:?}"
    );
}

#[test]
fn truncated_or_corrupted_module_is_rejected() {
    let bytes = try_emit("Q = (program (_) @name)").expect("valid query emits");

    Module::validate_and_load(&bytes).expect("pristine representation validates");

    for cut in [0, 1, 63, 64, 96, bytes.len() / 2, bytes.len() - 1] {
        assert!(
            Module::validate_and_load(&bytes[..cut]).is_err(),
            "truncation to {cut} bytes must be rejected"
        );
    }

    for i in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[i] ^= 0xFF;
        assert!(
            Module::validate_and_load(&corrupt).is_err(),
            "flipping byte {i} must be rejected"
        );
    }
}

#[test]
fn crafted_header_blob_size_does_not_overflow_offsets() {
    let mut bytes = try_emit("Q = (program (_) @name)").expect("valid query emits");
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());

    assert!(
        Module::validate_and_load(&bytes).is_err(),
        "near-u32::MAX str_blob_size must error, not overflow-panic"
    );
}
