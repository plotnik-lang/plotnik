use crate::compiler::srcgen::lits::{decimal_byte_lines, rust_string};

#[test]
fn rust_strings_use_language_native_escaping() {
    assert_eq!(rust_string("a\n\"b"), "\"a\\n\\\"b\"");
}

#[test]
fn decimal_bytes_have_stable_rows() {
    assert_eq!(
        decimal_byte_lines(&[1, 2, 3, 4, 5], 3, "    "),
        "    1, 2, 3,\n    4, 5,\n"
    );
}
