use std::path::Path;

use plotnik_langs::Lang;

/// Resolve language from explicit flag or infer from workspace directory name.
///
/// Directory inference: `queries.ts/` → typescript, `queries.javascript/` → javascript
pub fn resolve_lang(explicit: Option<&str>, query_path: Option<&Path>) -> Option<Lang> {
    // Explicit flag takes precedence
    if let Some(name) = explicit {
        return plotnik_langs::from_name(name);
    }

    // Infer from directory name extension: "queries.ts" → "ts"
    if let Some(path) = query_path
        && path.is_dir()
        && let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some((_, ext)) = name.rsplit_once('.')
    {
        return plotnik_langs::from_ext(ext);
    }

    None
}

/// Resolve language, returning an error message if unknown.
pub fn resolve_lang_required(lang_name: &str) -> Result<Lang, String> {
    plotnik_langs::from_name(lang_name).ok_or_else(|| format!("unknown language: '{}'", lang_name))
}

/// Resolve language with user-friendly error handling.
/// Tries explicit flag first, then infers from query path.
/// Exits with error message if language cannot be determined.
pub fn require_lang(
    explicit: Option<&str>,
    query_path: Option<&std::path::Path>,
    command: &str,
) -> Lang {
    if let Some(lang_name) = explicit {
        match resolve_lang_required(lang_name) {
            Ok(l) => return l,
            Err(msg) => {
                eprintln!("error: {}", msg);
                if let Some(suggestion) = suggest_language(lang_name) {
                    eprintln!();
                    eprintln!("Did you mean '{}'?", suggestion);
                }
                eprintln!();
                eprintln!("Run 'plotnik langs' for the full list.");
                std::process::exit(1);
            }
        }
    }

    if let Some(l) = resolve_lang(None, query_path) {
        return l;
    }

    eprintln!("error: language is required for {}", command);
    eprintln!();
    eprintln!("hint: use -l <language> to specify the target language");
    std::process::exit(1);
}

/// Suggest similar language names for typos.
pub fn suggest_language(input: &str) -> Option<String> {
    let input_lower = input.to_lowercase();
    plotnik_langs::all()
        .into_iter()
        .filter(|lang| levenshtein(lang.name(), &input_lower) <= 2)
        .min_by_key(|lang| levenshtein(lang.name(), &input_lower))
        .map(|lang| lang.name().to_string())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}
