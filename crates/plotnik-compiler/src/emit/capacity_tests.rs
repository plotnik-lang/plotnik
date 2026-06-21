//! Capacity limits and load-time validation for the bytecode boundary (#422).
//!
//! These assert that a query which would overflow a fixed-width field fails
//! emission with a clear [`EmitError`] instead of silently wrapping or panicking,
//! and that a truncated or corrupted module is rejected by `Module::load`.

use std::fmt::Write as _;
use std::sync::LazyLock;

use plotnik_bytecode::{EncodeError, Module};
use plotnik_core::grammar::{Grammar, raw::RawGrammar};

use crate::SourceMap;
use crate::emit::EmitError;
use crate::query::QueryBuilder;

fn javascript() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(include_str!(env!(
            "PLOTNIK_COMPILER_JAVASCRIPT_GRAMMAR_JSON"
        )))
        .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });
    &GRAMMAR
}

/// Link `src` (which must be valid — capacity limits live at emit, not link) and
/// return the emission result.
#[track_caller]
fn try_emit(src: &str) -> Result<Vec<u8>, EmitError> {
    let mut source_map = SourceMap::new();
    source_map.add_file("query.ptk", src);
    let query = QueryBuilder::new(source_map)
        .parse()
        .expect("query parses")
        .analyze()
        .link(javascript());
    assert!(query.is_valid(), "query should link:\n{src}");
    query.emit()
}

#[test]
fn struct_field_count_overflow_is_emit_error() {
    // 300 captures inside one node → a struct with 300 fields, past the u8 limit.
    let mut query = String::from("Q = (program");
    for i in 0..300 {
        write!(query, " (_) @c{i}").unwrap();
    }
    query.push(')');

    let err = try_emit(&query).expect_err("300 struct fields must not encode");
    assert!(matches!(err, EmitError::TooManyFields(300)), "got {err:?}");
}

#[test]
fn enum_variant_count_overflow_is_emit_error() {
    // 256 enum branches → an enum with 256 variants, past the u8 limit.
    let mut query = String::from("Q = (program [");
    for i in 0..256 {
        write!(query, " L{i}: (identifier) @v{i}").unwrap();
    }
    query.push_str("])");

    let err = try_emit(&query).expect_err("256 enum variants must not encode");
    assert!(
        matches!(err, EmitError::TooManyVariants(256)),
        "got {err:?}"
    );
}

#[test]
fn effect_member_payload_overflow_is_emit_error() {
    // Members are indexed into a 10-bit effect payload (max 1023). Spread > 1024
    // globally-distinct captures across several definitions (each struct staying
    // under the 255-field limit) so a Set effect references an index past 1023.
    let mut query = String::new();
    for def in 0..5 {
        write!(query, "D{def} = (program").unwrap();
        for field in 0..250 {
            write!(query, " (_) @d{def}_f{field}").unwrap();
        }
        query.push_str(")\n");
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

    // The pristine module loads.
    Module::load(&bytes).expect("pristine module loads");

    // Any truncation shorter than the whole file is rejected, never panics.
    for cut in [0, 1, 63, 64, 96, bytes.len() / 2, bytes.len() - 1] {
        assert!(
            Module::load(&bytes[..cut]).is_err(),
            "truncation to {cut} bytes must be rejected"
        );
    }

    // Flipping any single byte is caught: the CRC covers everything after the
    // 64-byte header, and structural checks (bounds, sentinels, reserved bytes,
    // type defs, entrypoints) cover the header itself.
    for i in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[i] ^= 0xFF;
        assert!(
            Module::load(&corrupt).is_err(),
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
        Module::load(&bytes).is_err(),
        "near-u32::MAX str_blob_size must error, not overflow-panic"
    );
}
