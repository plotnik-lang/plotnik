pub(crate) use plotnik_core::utils::{to_pascal_case, to_snake_case};

/// PascalCase begins with an ASCII uppercase letter; that first letter alone
/// decides identifier intent (reference/definition/label vs node/field name).
pub(crate) fn starts_uppercase(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
}
