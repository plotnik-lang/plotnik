use std::io::{self, Write as _};
use std::process::Command;

pub(crate) fn capture(command: &mut Command, context: &str) -> Result<Vec<u8>, String> {
    let rendered = format!("{command:?}");
    let output = command
        .output()
        .map_err(|error| format!("{context}: failed to start {rendered}: {error}"))?;
    if output.status.success() {
        if !output.stderr.is_empty() {
            io::stderr()
                .write_all(&output.stderr)
                .map_err(|error| format!("write subprocess stderr: {error}"))?;
        }
        return Ok(output.stdout);
    }
    Err(format!(
        "{context}\ncommand: {rendered}\nstatus: {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    ))
}
