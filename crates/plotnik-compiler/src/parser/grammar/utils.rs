pub(crate) use plotnik_core::utils::{to_pascal_case, to_snake_case};

pub(crate) fn capitalize_first(s: &str) -> String {
    assert!(!s.is_empty(), "capitalize_first: called with empty string");
    let mut chars = s.chars();
    let c = chars.next().unwrap();
    c.to_uppercase().chain(chars).collect()
}
