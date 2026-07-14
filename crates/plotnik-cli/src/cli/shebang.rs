//! Shebang parsing: the in-file language declaration for `.ptk` files.
//!
//! Line 1 of a query file may declare its language (and optionally an
//! entry point) via a shebang, e.g.:
//!
//! ```text
//! #!/usr/bin/env -S plotnik run -l typescript
//! ```
//!
//! Parsing rule: **lenient prefix, strict suffix**. Everything before the
//! `plotnik` token (interpreter path, `env -S`, …) is ignored; everything
//! after it must clap-parse against the unified run-family flag vocabulary.
//! Only semantic options (`-l`, `--entry`) are extracted — presentation
//! flags are accepted and ignored unless executing.

use clap::Command;

use super::args::*;
use super::limits::{fuel_arg, limits_preset_arg, max_memory_arg};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShebangDecl {
    pub lang: Option<String>,
    pub entry: Option<String>,
}

const SUBCOMMANDS: &[&str] = &[
    "run", "exec", "check", "infer", "tree", "trace", "dump", "test",
];

const CANONICAL_FORM: &str = "#!/usr/bin/env -S plotnik run -l <lang>";

/// Extract semantic options from the first line of query source.
///
/// Returns `Ok(None)` when the line is not a shebang or doesn't invoke
/// `plotnik`. Returns `Err` when options after the `plotnik` token are
/// malformed — that's a loud, line-1 error by design.
pub fn parse_shebang(source: &str) -> Result<Option<ShebangDecl>, String> {
    let first_line = source.lines().next().unwrap_or("");
    let Some(rest) = first_line.strip_prefix("#!") else {
        return Ok(None);
    };

    let mut tokens = rest.split_whitespace();
    if !tokens.any(|tok| tok == "plotnik" || tok.ends_with("/plotnik")) {
        return Ok(None);
    }

    let mut suffix: Vec<&str> = tokens.collect();
    if suffix.first().is_some_and(|tok| SUBCOMMANDS.contains(tok)) {
        suffix.remove(0);
    }

    let matches = shebang_parser().try_get_matches_from(suffix).map_err(|e| {
        format!(
            "{}\n\nexpected form: {}",
            first_error_line(&e),
            CANONICAL_FORM
        )
    })?;

    Ok(Some(ShebangDecl {
        lang: matches.get_one::<String>("lang").cloned(),
        entry: matches.get_one::<String>("entry").cloned(),
    }))
}

// No positional args: the file IS the query, so positionals would always be wrong.
fn shebang_parser() -> Command {
    Command::new("plotnik")
        .no_binary_name(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(lang_arg())
        .arg(entry_arg())
        .arg(color_arg())
        .arg(strict_arg())
        .arg(json_arg())
        .arg(compact_arg())
        .arg(include_points_arg())
        .arg(verbose_arg())
        .arg(no_result_arg())
        .arg(fuel_arg())
        .arg(max_memory_arg())
        .arg(limits_preset_arg())
        .arg(format_arg())
        .arg(no_node_type_arg())
        .arg(no_export_arg())
        .arg(match_only_type_arg())
        .arg(output_file_arg())
}

fn first_error_line(err: &clap::Error) -> String {
    let rendered = err.to_string();
    let line = rendered.lines().next().unwrap_or("invalid options");
    line.strip_prefix("error: ").unwrap_or(line).to_string()
}
