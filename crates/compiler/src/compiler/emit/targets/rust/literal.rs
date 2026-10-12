//! Rust literal and byte-blob rendering used by the shipped backend.

/// A Rust string literal with the standard library's deterministic escaping.
pub(crate) fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

/// Decimal byte-array rows with a fixed element count and indentation.
pub(crate) fn decimal_byte_lines(bytes: &[u8], width: usize, indent: &str) -> String {
    assert!(width > 0, "byte row width must be non-zero");
    let mut out = String::new();
    for chunk in bytes.chunks(width) {
        out.push_str(indent);
        out.push_str(
            &chunk
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(",\n");
    }
    out
}
