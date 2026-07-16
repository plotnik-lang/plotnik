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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn capture_reports_context_command_status_and_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'generated badly' >&2; exit 7"]);

        let error = capture(&mut command, "generate snapshot `bad.txt`").unwrap_err();

        assert!(error.contains("generate snapshot `bad.txt`"));
        assert!(error.contains("command:"));
        assert!(error.contains("status: exit status: 7"));
        assert!(error.contains("generated badly"));
    }
}
