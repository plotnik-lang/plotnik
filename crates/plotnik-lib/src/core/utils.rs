fn is_separator(c: char) -> bool {
    matches!(c, '_' | '-' | '.')
}

/// A new word starts at a lower/digit→upper transition (`fooBar`) or at the
/// last capital of an acronym run followed by lowercase (`HTTPServer` →
/// `HTTP` + `Server`).
fn is_word_boundary(prev: char, cur: char, next: Option<char>) -> bool {
    cur.is_ascii_uppercase()
        && (prev.is_ascii_lowercase()
            || prev.is_ascii_digit()
            || (prev.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase())))
}

/// Convert snake_case, kebab-case, or camelCase to PascalCase.
///
/// Words are split on `_`, `-`, `.`, and camel boundaries (see
/// [`is_word_boundary`]); each word is capitalized and the rest lowercased,
/// so `foo_bar`, `fooBar`, and `FOO_BAR` all become `FooBar`, and
/// `HTTPServer` becomes `HttpServer`. Idempotent on PascalCase input.
pub fn to_pascal_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut word_start = true;
    let mut prev: Option<char> = None;
    for (i, &c) in chars.iter().enumerate() {
        if is_separator(c) {
            word_start = true;
            prev = None;
            continue;
        }
        if let Some(p) = prev
            && is_word_boundary(p, c, chars.get(i + 1).copied())
        {
            word_start = true;
        }
        if word_start {
            result.push(c.to_ascii_uppercase());
            word_start = false;
        } else {
            result.push(c.to_ascii_lowercase());
        }
        prev = Some(c);
    }
    result
}

/// Convert PascalCase or camelCase to snake_case.
///
/// Acronym runs stay one word: `HTTPServer` becomes `http_server`, not
/// `h_t_t_p_server`. Existing separators pass through unchanged.
pub fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0
            && is_word_boundary(chars[i - 1], c, chars.get(i + 1).copied())
            && !result.ends_with('_')
        {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Simple edit distance for fuzzy matching (Levenshtein).
///
/// This is optimized for correctness and small inputs (identifiers, field names),
/// not for very large strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the best match from candidates within the shared suggestion threshold.
///
/// Returns the closest candidate (lowest distance) if it is within the threshold.
pub fn find_similar<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let max_distance = (name.len() / 3).clamp(2, 4);
    candidates
        .iter()
        .map(|&c| (c, edit_distance(name, c)))
        .filter(|(_, d)| *d <= max_distance)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}
