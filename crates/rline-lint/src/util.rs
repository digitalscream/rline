//! Shared helpers for spawning external tools.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::error::LintError;

/// Run `cmd` with `args` in `cwd`, optionally piping `stdin_data` to it, and
/// return its `Output`.
///
/// Returns [`LintError::ToolNotFound`] if the binary cannot be located, and
/// [`LintError::Io`] for any other spawn/wait failure.
pub(crate) fn run_command(
    tool: &'static str,
    cmd: &str,
    args: &[&str],
    cwd: Option<&Path>,
    stdin_data: Option<&str>,
) -> Result<Output, LintError> {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            LintError::ToolNotFound(cmd.to_owned())
        } else {
            LintError::Io {
                tool: tool.to_owned(),
                source: e,
            }
        }
    })?;

    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(data.as_bytes())
                .map_err(|e| LintError::Io {
                    tool: tool.to_owned(),
                    source: e,
                })?;
            // Drop closes stdin so the child sees EOF.
        }
    }

    child.wait_with_output().map_err(|e| LintError::Io {
        tool: tool.to_owned(),
        source: e,
    })
}

/// Convert `output.stdout` to a String, returning [`LintError::InvalidUtf8`]
/// on bad bytes.
pub(crate) fn stdout_string(tool: &'static str, output: &Output) -> Result<String, LintError> {
    String::from_utf8(output.stdout.clone()).map_err(|e| LintError::InvalidUtf8 {
        tool: tool.to_owned(),
        source: e,
    })
}

/// Trimmed stderr as a String, lossy-decoded so it can never fail.
pub(crate) fn stderr_lossy(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// Build a [`LintError::ToolFailed`] from a finished process whose exit was
/// not what we expected.
pub(crate) fn tool_failed(tool: &'static str, output: &Output) -> LintError {
    LintError::ToolFailed {
        tool: tool.to_owned(),
        code: output.status.code(),
        stderr: stderr_lossy(output),
    }
}
