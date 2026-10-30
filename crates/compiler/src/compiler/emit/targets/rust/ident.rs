use std::collections::HashSet;

pub(crate) fn shouty_ident(name: &str) -> String {
    case_segments(name)
        .map(|segment| segment.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

pub(crate) fn snake_ident(name: &str) -> String {
    case_segments(name)
        .map(|segment| segment.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

const RUST_UNRAW: &[&str] = &["crate", "self", "super", "Self"];

const RUST_RAW: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "typeof", "unsafe", "use", "virtual",
    "where", "while", "yield",
];

pub(crate) fn module_ident(stem: &str) -> Result<String, String> {
    let mut chars = stem.chars();
    let valid = chars
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
    if !valid || stem == "_" || stem == "mod" || RUST_UNRAW.contains(&stem) {
        return Err(format!(
            "invalid Rust module name `{stem}`. Use a lowercase snake_case query filename. `_`, `mod`, `self`, `super`, and `crate` are reserved."
        ));
    }

    Ok(rust_ident(stem))
}

pub(crate) fn rust_scope_idents<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut taken = HashSet::new();
    names
        .map(|name| {
            let mut ident = rust_ident(name);
            while !taken.insert(ident.clone()) {
                ident.push('_');
            }
            ident
        })
        .collect()
}

fn rust_ident(name: &str) -> String {
    if RUST_UNRAW.contains(&name) {
        format!("{name}_")
    } else if RUST_RAW.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

fn case_segments(name: &str) -> impl Iterator<Item = String> {
    let chars: Vec<char> = name.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous_is_upper = index > 0 && chars[index - 1].is_uppercase();
        let next_is_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
        let boundary =
            ch.is_uppercase() && !current.is_empty() && (!previous_is_upper || next_is_lower);
        if boundary {
            segments.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments.into_iter()
}
