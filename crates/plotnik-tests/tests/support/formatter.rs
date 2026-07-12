use plotnik_lib::{FormatError, TokenSpan, format_query, tokenize};
use similar::TextDiff;

pub enum Assessment {
    Unformattable,
    Unchanged(String),
    Changed(String),
}

impl Assessment {
    pub fn into_query_or(self, authored: String) -> String {
        match self {
            Self::Unchanged(query) | Self::Changed(query) => query,
            Self::Unformattable => authored,
        }
    }
}

pub fn evaluate(query: &str, name: &str) -> Result<Assessment, String> {
    let output = match format_query(query) {
        Ok(output) => output,
        Err(FormatError::Parse { .. }) => return Ok(Assessment::Unformattable),
        Err(error) => return Err(format!("format query for `{name}`: {error}")),
    };
    let repeated = format_query(&output).map_err(|error| {
        format!("formatted query for `{name}` did not parse on the second pass: {error}")
    })?;
    if repeated != output {
        return Err(format!(
            "formatter is not idempotent for `{name}`:\n{}",
            unified_diff(&output, &repeated)
        ));
    }
    assert_contract(query, &output, name)?;

    let formatted = output
        .strip_suffix('\n')
        .expect("successful formatter output ends in one newline");
    if formatted.ends_with('\n') {
        return Err(format!(
            "formatter emitted more than one trailing newline for `{name}`"
        ));
    }
    if query == formatted {
        return Ok(Assessment::Unchanged(formatted.to_owned()));
    }
    Ok(Assessment::Changed(formatted.to_owned()))
}

fn assert_contract(input: &str, output: &str, name: &str) -> Result<(), String> {
    let input_tokens = tokenize(input);
    let output_tokens = tokenize(output);
    let input_significant = significant_token_signature(input, &input_tokens);
    let output_significant = significant_token_signature(output, &output_tokens);
    if input_significant != output_significant {
        return Err(format!(
            "formatter changed significant tokens for `{name}`:\ninput:  {input_significant:?}\noutput: {output_significant:?}"
        ));
    }

    let input_comments = comment_signature(input, &input_tokens);
    let output_comments = comment_signature(output, &output_tokens);
    if input_comments != output_comments {
        return Err(format!(
            "formatter changed comments for `{name}`:\ninput:  {input_comments:?}\noutput: {output_comments:?}"
        ));
    }

    let mut line_starts = vec![0];
    line_starts.extend(
        output
            .match_indices('\n')
            .map(|(newline, _)| newline + 1)
            .filter(|start| *start < output.len()),
    );
    let mut captures_per_line = vec![0; output.lines().count()];
    for token in output_tokens.iter().filter(|token| token.kind == "capture") {
        let line_index = line_starts.partition_point(|start| *start <= token.start as usize) - 1;
        captures_per_line[line_index] += 1;
    }
    for (line_index, captures) in captures_per_line.into_iter().enumerate() {
        if captures <= 1 {
            continue;
        }
        let line = output.lines().nth(line_index).expect("capture line exists");
        return Err(format!(
            "formatter emitted {captures} captures on line {} of `{name}`: {line}",
            line_index + 1
        ));
    }
    Ok(())
}

fn significant_token_signature(source: &str, tokens: &[TokenSpan]) -> Vec<(String, String)> {
    tokens
        .iter()
        .filter(|token| !matches!(token.kind, "whitespace" | "comment"))
        .map(|token| {
            let text = &source[token.start as usize..token.end as usize];
            let normalized = match text {
                "/" => "#",
                "'" => "\"",
                _ => text,
            };
            (token.kind.to_owned(), normalized.to_owned())
        })
        .collect()
}

fn comment_signature(source: &str, tokens: &[TokenSpan]) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| token.kind == "comment")
        .map(|token| {
            let text = &source[token.start as usize..token.end as usize];
            if text.starts_with("//") || text.starts_with(';') {
                return text.trim_end_matches([' ', '\t']).to_owned();
            }
            text.replace("\r\n", "\n").replace('\r', "\n")
        })
        .collect()
}

fn unified_diff(actual: &str, expected: &str) -> String {
    TextDiff::from_lines(actual, expected)
        .unified_diff()
        .context_radius(3)
        .header("first pass", "second pass")
        .to_string()
}
