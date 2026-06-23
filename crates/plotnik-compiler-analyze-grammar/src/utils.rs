//! Small string utilities shared by query passes.
//!
//! This module intentionally stays minimal and dependency-free.
//! Only extract helpers here when they are used by 2+ modules or are clearly
//! pass-agnostic (formatting, suggestion, small string algorithms).

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

/// Find the best match from candidates within a maximum edit distance.
///
/// Returns the closest candidate (lowest distance) if it is within `max_distance`.
pub fn find_similar<'a>(
    name: &str,
    candidates: &[&'a str],
    max_distance: usize,
) -> Option<&'a str> {
    candidates
        .iter()
        .map(|&c| (c, edit_distance(name, c)))
        .filter(|(_, d)| *d <= max_distance)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}
