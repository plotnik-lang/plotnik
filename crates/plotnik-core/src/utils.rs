/// Convert snake_case or kebab-case to PascalCase.
///
/// Normalizes words separated by `_`, `-`, or `.`. If the input is already
/// PascalCase (starts uppercase, no separators), it is returned unchanged.
///
/// # Examples
/// ```
/// use plotnik_core::utils::to_pascal_case;
/// assert_eq!(to_pascal_case("foo_bar"), "FooBar");
/// assert_eq!(to_pascal_case("FOO_BAR"), "FooBar");
/// assert_eq!(to_pascal_case("FooBar"), "FooBar");  // idempotent
/// ```
pub fn to_pascal_case(s: &str) -> String {
    fn is_separator(c: char) -> bool {
        matches!(c, '_' | '-' | '.')
    }

    let has_separator = s.chars().any(is_separator);
    let has_lowercase = s.chars().any(|c| c.is_ascii_lowercase());
    let starts_uppercase = s.chars().next().is_some_and(|c| c.is_ascii_uppercase());

    // Already PascalCase: starts uppercase, has lowercase, no separators
    if starts_uppercase && has_lowercase && !has_separator {
        return s.to_string();
    }

    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if is_separator(c) {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

/// Convert PascalCase or camelCase to snake_case.
///
/// # Examples
/// ```
/// use plotnik_core::utils::to_snake_case;
/// assert_eq!(to_snake_case("FooBar"), "foo_bar");
/// assert_eq!(to_snake_case("fooBar"), "foo_bar");
/// ```
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 && !result.ends_with('_') {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}
