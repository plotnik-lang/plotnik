//! Capacity limits and load-time validation for the bytecode boundary (#422).
//!
//! These assert that a query which would overflow a fixed-width field fails
//! emission with a clear [`EmitError`] instead of silently wrapping or panicking,
//! and that a truncated or corrupted module is rejected by `Module::validate_and_load`.

use std::fmt::Write as _;

use crate::bytecode::{EncodeError, Module};

use super::EmitError;
use crate::compiler::query::QueryBuilder;
use crate::compiler::test_utils::synthetic_grammar as grammar;
use crate::compiler::{BytecodeConfig, DiagnosticKind, RustCodegenConfig, TypeScriptCodegenConfig};
use crate::compiler::{SourceMap, SourcePath};

/// Bind `src` to the test grammar (capacity limits live at emission, not binding) and
/// return the emission result.
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
        write!(query, " (expression_statement) @field_{index}").unwrap();
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
    assert!(
        compiled
            .emit_types(TypeScriptCodegenConfig::new())
            .expect("TypeScript type emission answers")
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
    assert!(
        compiled
            .emit_types(TypeScriptCodegenConfig::new())
            .expect("TypeScript type emission answers")
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
    // Members are indexed into a 10-bit effect payload (max 1023). Spread > 1024
    // globally-distinct captures across several definitions (each record staying
    // under the 255-field limit) so a RecordSet effect references an index past 1023.
    let mut query = String::new();
    for def in 0..5 {
        write!(query, "D{def} = (program").unwrap();
        for field in 0..250 {
            // Concrete child kind: keeps each definition's satisfiability solve linear
            // (the wide member count is the point, not wildcard matching).
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

    // The pristine representation validates.
    Module::validate_and_load(&bytes).expect("pristine representation validates");

    // Any truncation shorter than the whole file is rejected, never panics.
    for cut in [0, 1, 63, 64, 96, bytes.len() / 2, bytes.len() - 1] {
        assert!(
            Module::validate_and_load(&bytes[..cut]).is_err(),
            "truncation to {cut} bytes must be rejected"
        );
    }

    // Flipping any single byte is caught: the CRC covers everything after the
    // 64-byte header, and structural checks (bounds, sentinels, reserved bytes,
    // type defs, entry points) cover the header itself.
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
    // `str_blob_size` lives in the header (bytes 16..20), which the CRC does not
    // cover, so a corrupt near-`u32::MAX` value reaches offset computation. It
    // must be rejected by the section-bounds check, not overflow the u32
    // arithmetic in `compute_offsets` (a debug panic — load failing open).
    let mut bytes = try_emit("Q = (program (_) @name)").expect("valid query emits");
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());

    assert!(
        Module::validate_and_load(&bytes).is_err(),
        "near-u32::MAX str_blob_size must error, not overflow-panic"
    );
}
