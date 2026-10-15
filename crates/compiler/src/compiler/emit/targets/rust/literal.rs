pub(crate) fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

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
