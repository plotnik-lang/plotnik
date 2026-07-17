//! Shared parsing for snapshot documents and section boundaries.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub query: String,
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub body: String,
}

pub fn parse_document(raw: &str) -> Result<Document, String> {
    let normalized = raw.replace("\r\n", "\n");
    let mut query_lines = Vec::new();
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;

    for line in normalized.lines() {
        if let Some(name) = parse_section_header(line) {
            if let Some(previous) = current.take() {
                sections.push(previous);
            }
            current = Some((name, Vec::new()));
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            body.push(line);
            continue;
        }
        query_lines.push(line);
    }
    if let Some(previous) = current {
        sections.push(previous);
    }

    let query = query_lines.join("\n");
    if query.trim().is_empty() {
        return Err("snapshot has no query (text before the first `--- … ---` rule)".into());
    }
    let sections = sections
        .into_iter()
        .map(|(name, body)| Section {
            name,
            body: body.join("\n"),
        })
        .collect();
    Ok(Document { query, sections })
}

/// A known snapshot section at column zero, normalized to the harness's internal
/// name. Unknown dash-padded source lines are snapshot data, not boundaries.
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
/// (` LABEL `), exactly as the snapshot test harness emits it; requiring that padding
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
