//! Permission checking for agent tool calls.
//!
//! Determines whether a tool call should be auto-approved based on the
//! tool category, target path, and user settings.

use std::path::Path;

use rline_ai::tools::ToolCategory;
use rline_config::EditorSettings;

/// Check whether a tool call should be auto-approved.
///
/// Returns `true` if the tool can execute without user confirmation.
pub fn should_auto_approve(
    _tool_name: &str,
    category: ToolCategory,
    arguments: &str,
    workspace_root: &Path,
    settings: &EditorSettings,
) -> bool {
    match category {
        ToolCategory::ReadFile => {
            if !settings.agent_auto_approve_read {
                return false;
            }
            // Auto-approve only if the target path is within the workspace.
            is_path_in_workspace(arguments, workspace_root)
        }
        ToolCategory::EditFile => {
            if !settings.agent_auto_approve_edit {
                return false;
            }
            is_path_in_workspace(arguments, workspace_root)
        }
        ToolCategory::ExecuteCommand => {
            if !settings.agent_auto_approve_command {
                return false;
            }
            // Only auto-approve commands classified as safe.
            is_safe_command(arguments)
        }
        // Interactive tools (ask_followup_question, attempt_completion) are always
        // handled specially by the agent loop — they don't need approval.
        ToolCategory::Interactive => true,
    }
}

/// Try to extract a "path" field from JSON arguments and check if it
/// resolves to a location within the workspace root.
fn is_path_in_workspace(arguments: &str, workspace_root: &Path) -> bool {
    let path_str = match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(v) => v.get("path").and_then(|p| p.as_str()).map(String::from),
        Err(_) => None,
    };

    let Some(path_str) = path_str else {
        // No path field — can't verify workspace containment.
        // Default to requiring approval for safety.
        return false;
    };

    let resolved = if Path::new(&path_str).is_absolute() {
        Path::new(&path_str).to_path_buf()
    } else {
        workspace_root.join(&path_str)
    };

    // Canonicalize both paths to resolve symlinks and `..`.
    let resolved = resolved.canonicalize().unwrap_or(resolved);
    let root = workspace_root
        .canonicalize()
        .unwrap_or(workspace_root.to_path_buf());

    resolved.starts_with(&root)
}

/// Check whether a shell command is considered safe (read-only / non-destructive).
///
/// Uses the same classification as Cline: a whitelist of safe command prefixes
/// combined with rejection of shell operators that could chain destructive
/// operations.
fn is_safe_command(arguments: &str) -> bool {
    let cmd = match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(v) => v.get("command").and_then(|c| c.as_str()).map(String::from),
        Err(_) => None,
    };

    let Some(cmd) = cmd else {
        return false;
    };

    let cmd = cmd.trim();

    // Reject commands that use shell operators to chain potentially
    // destructive operations — pipes to safe commands are still flagged
    // because the left side could be anything.
    if contains_shell_operator(cmd) {
        return false;
    }

    // Strip leading env var assignments (e.g. `FOO=bar cmd ...`).
    let effective = strip_env_assignments(cmd);

    SAFE_COMMAND_PREFIXES.iter().any(|prefix| {
        // Multi-word prefixes like "cargo build" — match the start of the command.
        if prefix.contains(' ') {
            effective == *prefix || effective.starts_with(&format!("{prefix} "))
        } else {
            // Single-word — match the first token exactly.
            let base = effective.split_whitespace().next().unwrap_or("");
            base == *prefix || base.ends_with(&format!("/{prefix}"))
        }
    })
}

/// Whether the command string contains shell chaining / redirection operators.
fn contains_shell_operator(cmd: &str) -> bool {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev = '\0';
    for ch in cmd.chars() {
        match ch {
            '\'' if !in_double_quote && prev != '\\' => in_single_quote = !in_single_quote,
            '"' if !in_single_quote && prev != '\\' => in_double_quote = !in_double_quote,
            '|' | ';' | '>' | '<' | '&' if !in_single_quote && !in_double_quote => {
                return true;
            }
            _ => {}
        }
        prev = ch;
    }
    false
}

/// Strip leading environment variable assignments like `FOO=bar cmd args`.
fn strip_env_assignments(cmd: &str) -> &str {
    let mut remaining = cmd;
    loop {
        let trimmed = remaining.trim_start();
        match trimmed.split_once(char::is_whitespace) {
            Some((first, rest)) if first.contains('=') => remaining = rest,
            _ => return trimmed,
        }
    }
}

/// Command prefixes considered safe (read-only / non-destructive).
///
/// Matches Cline's safe command classification.
const SAFE_COMMAND_PREFIXES: &[&str] = &[
    // ── Filesystem inspection ──
    "ls",
    "dir",
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "find",
    "fd",
    "tree",
    "file",
    "stat",
    "wc",
    "du",
    "df",
    "pwd",
    "realpath",
    "basename",
    "dirname",
    // ── Text search ──
    "grep",
    "rg",
    "ag",
    "ack",
    "sed", // note: without -i, sed is read-only — but we flag pipes anyway
    "awk",
    "sort",
    "uniq",
    "diff",
    "comm",
    "cut",
    "tr",
    "tee",
    // ── System info ──
    "echo",
    "printf",
    "whoami",
    "which",
    "where",
    "type",
    "env",
    "printenv",
    "uname",
    "hostname",
    "date",
    "uptime",
    "free",
    "lsb_release",
    "id",
    // ── Version / help ──
    "man",
    "help",
    // ── Git (read-only operations) ──
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "git tag",
    "git remote",
    "git stash list",
    "git ls-files",
    "git blame",
    "git shortlog",
    "git rev-parse",
    "git describe",
    "git config --get",
    "git config --list",
    // ── Build / test / lint (produce output, don't modify source) ──
    "cargo build",
    "cargo test",
    "cargo check",
    "cargo clippy",
    "cargo fmt --check",
    "cargo doc",
    "cargo bench",
    "cargo run",
    "cargo tree",
    "cargo metadata",
    "rustc",
    "make",
    "cmake",
    "go build",
    "go test",
    "go vet",
    "go run",
    "npm test",
    "npm run",
    "npm ls",
    "npm list",
    "npm outdated",
    "npm info",
    "npx",
    "yarn test",
    "yarn run",
    "yarn list",
    "yarn info",
    "pnpm test",
    "pnpm run",
    "pip list",
    "pip show",
    "pip freeze",
    "python -c",
    "python3 -c",
    "python -m pytest",
    "python3 -m pytest",
    "node -e",
    "node -p",
    "ruby -e",
    "gcc",
    "g++",
    "clang",
    "javac",
    "java",
    "dotnet build",
    "dotnet test",
    "dotnet run",
    // ── Misc safe tools ──
    "jq",
    "yq",
    "xargs",
    "true",
    "false",
    "test",
    "tput",
    "clear",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings(read: bool, edit: bool, cmd: bool) -> EditorSettings {
        let mut s = EditorSettings::default();
        s.agent_auto_approve_read = read;
        s.agent_auto_approve_edit = edit;
        s.agent_auto_approve_command = cmd;
        s
    }

    #[test]
    fn test_auto_approve_read_in_workspace() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        std::fs::write(dir.path().join("test.txt"), "").expect("write in test");
        let settings = test_settings(true, false, false);
        let args = r#"{"path": "test.txt"}"#;

        assert!(
            should_auto_approve(
                "read_file",
                ToolCategory::ReadFile,
                args,
                dir.path(),
                &settings
            ),
            "should auto-approve read_file in workspace"
        );
    }

    #[test]
    fn test_no_approve_read_disabled() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let settings = test_settings(false, false, false);
        let args = r#"{"path": "test.txt"}"#;

        assert!(
            !should_auto_approve(
                "read_file",
                ToolCategory::ReadFile,
                args,
                dir.path(),
                &settings
            ),
            "should not auto-approve when read is disabled"
        );
    }

    #[test]
    fn test_no_approve_edit_disabled() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let settings = test_settings(true, false, false);
        let args = r#"{"path": "test.txt", "content": "hi"}"#;

        assert!(
            !should_auto_approve(
                "write_to_file",
                ToolCategory::EditFile,
                args,
                dir.path(),
                &settings
            ),
            "should not auto-approve when edit is disabled"
        );
    }

    #[test]
    fn test_safe_command_ls() {
        let args = r#"{"command": "ls -la"}"#;
        assert!(is_safe_command(args), "ls should be safe");
    }

    #[test]
    fn test_safe_command_cargo_test() {
        let args = r#"{"command": "cargo test --workspace"}"#;
        assert!(is_safe_command(args), "cargo test should be safe");
    }

    #[test]
    fn test_safe_command_git_status() {
        let args = r#"{"command": "git status"}"#;
        assert!(is_safe_command(args), "git status should be safe");
    }

    #[test]
    fn test_unsafe_command_rm() {
        let args = r#"{"command": "rm -rf /tmp/stuff"}"#;
        assert!(!is_safe_command(args), "rm should be unsafe");
    }

    #[test]
    fn test_unsafe_command_git_push() {
        let args = r#"{"command": "git push origin main"}"#;
        assert!(!is_safe_command(args), "git push should be unsafe");
    }

    #[test]
    fn test_unsafe_command_pipe() {
        let args = r#"{"command": "cat file.txt | xargs rm"}"#;
        assert!(!is_safe_command(args), "piped commands should be unsafe");
    }

    #[test]
    fn test_unsafe_command_redirect() {
        let args = r#"{"command": "echo bad > important.txt"}"#;
        assert!(!is_safe_command(args), "redirect should be unsafe");
    }

    #[test]
    fn test_unsafe_command_chained() {
        let args = r#"{"command": "ls && rm -rf /"}"#;
        assert!(!is_safe_command(args), "chained commands should be unsafe");
    }

    #[test]
    fn test_safe_command_with_env_var() {
        let args = r#"{"command": "RUST_LOG=debug cargo test"}"#;
        assert!(
            is_safe_command(args),
            "env var prefix with safe command should be safe"
        );
    }

    #[test]
    fn test_safe_command_full_path() {
        let args = r#"{"command": "/usr/bin/grep foo bar.txt"}"#;
        assert!(
            is_safe_command(args),
            "full path to safe command should be safe"
        );
    }

    #[test]
    fn test_unsafe_command_sudo() {
        let args = r#"{"command": "sudo apt install foo"}"#;
        assert!(!is_safe_command(args), "sudo should be unsafe");
    }

    #[test]
    fn test_interactive_always_approved() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let settings = test_settings(false, false, false);
        let args = r#"{"question": "what?"}"#;

        assert!(
            should_auto_approve(
                "ask_followup_question",
                ToolCategory::Interactive,
                args,
                dir.path(),
                &settings
            ),
            "interactive tools should always be auto-approved"
        );
    }
}
