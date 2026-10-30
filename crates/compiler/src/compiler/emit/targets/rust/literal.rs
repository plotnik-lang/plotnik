pub(crate) fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

pub(super) fn rust_byte_string(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for &byte in bytes {
        out.extend(std::ascii::escape_default(byte).map(char::from));
    }
    out.push('"');
    out
}
