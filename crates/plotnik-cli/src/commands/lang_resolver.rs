use crate::error::CliError;
use crate::language_registry::{self, Lang};

/// Resolve a language name or alias, with typo suggestions on failure.
pub fn resolve_lang_name(name: &str) -> Result<&'static Lang, CliError> {
    language_registry::from_name(name).ok_or_else(|| {
        let mut msg = format!("unknown language: '{}'", name);
        if let Some(suggestion) = suggest_language(name) {
            msg.push_str(&format!("\n\nDid you mean '{}'?", suggestion));
        }
        msg.push_str("\n\nRun 'plotnik lang list' for the full list.");
        CliError::Fatal(msg)
    })
}

/// The shebang is the in-file language declaration; an explicit `-l` flag is
/// allowed but must agree with it.
pub fn reconcile_lang(
    explicit: Option<&str>,
    declared: Option<&str>,
) -> Result<Option<&'static Lang>, CliError> {
    match (explicit, declared) {
        (None, None) => Ok(None),
        (Some(name), None) => resolve_lang_name(name).map(Some),
        (None, Some(name)) => resolve_lang_name(name)
            .map_err(wrap_shebang_error)
            .map(Some),
        (Some(explicit_name), Some(declared_name)) => {
            let explicit_lang = resolve_lang_name(explicit_name)?;
            let declared_lang = resolve_lang_name(declared_name).map_err(wrap_shebang_error)?;
            if !std::ptr::eq(explicit_lang, declared_lang) {
                return Err(CliError::fatal(format!(
                    "-l {} conflicts with the shebang declaration '{}'",
                    explicit_name, declared_name
                )));
            }
            Ok(Some(explicit_lang))
        }
    }
}

fn wrap_shebang_error(err: CliError) -> CliError {
    match err {
        CliError::Fatal(msg) => CliError::Fatal(format!("in shebang declaration: {}", msg)),
        other => other,
    }
}

/// Resolve the language for commands that require one (check, dump, infer).
/// Priority: explicit `-l` (must agree with shebang) > shebang.
pub fn require_lang(
    explicit: Option<&str>,
    declared: Option<&str>,
    command: &str,
) -> Result<&'static Lang, CliError> {
    reconcile_lang(explicit, declared)?.ok_or_else(|| {
        CliError::fatal(format!(
            "language is required for {}\n\nhint: use -l <language> or declare it in the query shebang:\n  #!/usr/bin/env -S plotnik run -l <language>",
            command
        ))
    })
}

pub fn suggest_language(input: &str) -> Option<String> {
    let input_lower = input.to_lowercase();
    language_registry::all()
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
