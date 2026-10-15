pub(crate) use crate::core::utils::{to_pascal_case, to_snake_case};

pub(crate) fn starts_uppercase(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
}
