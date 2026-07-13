//! Shared parsing for golden-fixture section boundaries.

/// A known fixture section at column zero, normalized to the harness's internal
/// name. Unknown dash-padded source lines are fixture data, not boundaries.
pub fn parse_section_header(line: &str) -> Option<String> {
    let label = rule_label(line)?;
    let name = match label.split_once('(') {
        Some((head, ext)) if head.trim().eq_ignore_ascii_case("input") => {
            format!("input.{}", ext.strip_suffix(')')?.trim())
        }
        Some(_) => return None,
        None => label.to_ascii_lowercase(),
    };
    let known = name == "input"
        || name.starts_with("input.")
        || matches!(
            name.as_str(),
            "cst"
                | "ast"
                | "symbols"
                | "nfa"
                | "bytecode"
                | "mapped"
                | "typescript"
                | "rust"
                | "matcher"
                | "trace"
                | "output"
                | "inspection"
                | "execution_trace"
                | "diagnostics"
        );
    known.then_some(name)
}

/// The label inside a `----- LABEL -----` rule, or `None` when the line isn't a
/// rule. A rule sits at column zero and pads its label with a space on each side
/// (` LABEL `), exactly as the golden harness emits it; requiring that padding
/// keeps authored query and input bytes from becoming boundaries accidentally.
fn rule_label(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if !line.starts_with('-') || !line.ends_with('-') {
        return None;
    }
    let label = line
        .trim_matches('-')
        .strip_prefix(' ')?
        .strip_suffix(' ')?
        .trim();
    (!label.is_empty()).then_some(label)
}
