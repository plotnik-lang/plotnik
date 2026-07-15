use std::path::Path;
use std::process::Command;

use crate::corpus::Case;
use crate::process;

pub(crate) fn source(
    plotnik: &Path,
    target: &str,
    case: &Case,
    query_path: &Path,
) -> Result<String, String> {
    let mut command = Command::new(plotnik);
    command
        .arg("gen")
        .arg(query_path)
        .arg("--target")
        .arg(target)
        .arg("--grammar")
        .arg(&case.grammar_json)
        .arg("--debug")
        .arg("--color")
        .arg("never");
    let context = format!(
        "generate {target} fixture `{}` with grammar {}",
        case.relative,
        case.grammar_json.display()
    );
    let output = process::capture(&mut command, &context)?;
    String::from_utf8(output).map_err(|error| {
        format!(
            "generated {target} source for `{}` is not UTF-8: {error}",
            case.relative
        )
    })
}
