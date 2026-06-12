pub(crate) use plotnik_core::utils::{to_pascal_case, to_snake_case};

/// PascalCase begins with an ASCII uppercase letter; that first letter alone
/// decides identifier intent (reference/definition/label vs node/field name).
pub(crate) fn starts_uppercase(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
}

pub(crate) fn capitalize_first(s: &str) -> String {
    assert!(!s.is_empty(), "capitalize_first: called with empty string");
    let mut chars = s.chars();
    let c = chars.next().unwrap();
    c.to_uppercase().chain(chars).collect()
}
