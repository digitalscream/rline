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
            // Never auto-approve commands that affect the system outside the
            // project directory (apt, sudo, global installs, etc.).
            if is_system_affecting_command(arguments) {
                return false;
            }
            // Only auto-approve commands classified as safe.
            is_safe_command(arguments)
        }
        // Interactive tools (ask_followup_question, attempt_completion) are always
        // handled specially by the agent loop — they don't need approval.
        ToolCategory::Interactive => true,
        // Trusted MCP servers have their tools auto-approved.
        ToolCategory::McpTrusted => true,
        // Untrusted MCP servers always require explicit user approval.
        ToolCategory::McpUntrusted => false,
        // Browser actions don't touch the filesystem outside the workspace.
        ToolCategory::Browser => settings.agent_auto_approve_browser,
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

/// Check whether a command may affect the system outside the project directory.
///
/// Detects package managers that install system-wide, privilege escalation,
/// service management, and other commands with global side effects. These
/// always require explicit user approval.
pub fn is_system_affecting_command(arguments: &str) -> bool {
    let cmd = match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(v) => v.get("command").and_then(|c| c.as_str()).map(String::from),
        Err(_) => None,
    };

    let Some(cmd) = cmd else {
        // Can't parse — assume system-affecting for safety.
        return true;
    };

    let cmd = cmd.trim();

    // Strip leading env var assignments.
    let effective = strip_env_assignments(cmd);

    // Check the first token (the command name).
    let first_token = effective.split_whitespace().next().unwrap_or("");
    let base_cmd = first_token.rsplit('/').next().unwrap_or(first_token);

    // Privilege escalation — always system-affecting.
    if base_cmd == "sudo" || base_cmd == "doas" || base_cmd == "pkexec" {
        return true;
    }

    // System package managers.
    if SYSTEM_PACKAGE_MANAGERS.iter().any(|pm| base_cmd == *pm) {
        return true;
    }

    // npm/yarn/pnpm with -g or --global flag.
    if (base_cmd == "npm" || base_cmd == "yarn" || base_cmd == "pnpm")
        && (effective.contains(" -g ")
            || effective.contains(" --global")
            || effective.ends_with(" -g"))
    {
        return true;
    }

    // pip/pip3 install without --user or venv indicator.
    if (base_cmd == "pip" || base_cmd == "pip3") && effective.contains("install") {
        // If it has --user, --target, or -t, it's probably project-scoped.
        if !effective.contains("--user")
            && !effective.contains("--target")
            && !effective.contains(" -t ")
        {
            return true;
        }
    }

    // gem install (system-wide ruby gems).
    if base_cmd == "gem" && effective.contains("install") {
        return true;
    }

    // Service management.
    if base_cmd == "systemctl" || base_cmd == "service" || base_cmd == "launchctl" {
        return true;
    }

    // Docker/podman (can affect system containers).
    if base_cmd == "docker" || base_cmd == "podman" {
        return true;
    }

    // Firewall / network.
    if base_cmd == "iptables" || base_cmd == "ufw" || base_cmd == "firewall-cmd" {
        return true;
    }

    // User/group management.
    if base_cmd == "useradd"
        || base_cmd == "userdel"
        || base_cmd == "usermod"
        || base_cmd == "groupadd"
        || base_cmd == "chown"
        || base_cmd == "chmod"
    {
        // chmod/chown within the project dir is fine, but we can't easily
        // determine that from just the command string without path resolution.
        // Be conservative.
        return true;
    }

    false
}

/// System package managers that always install globally.
const SYSTEM_PACKAGE_MANAGERS: &[&str] = &[
    "apt", "apt-get", "dpkg", "yum", "dnf", "pacman", "zypper", "apk", "brew", "port", "snap",
    "flatpak",
];

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
        EditorSettings {
            agent_auto_approve_read: read,
            agent_auto_approve_edit: edit,
            agent_auto_approve_command: cmd,
            ..EditorSettings::default()
        }
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

    // ── System-affecting command tests ──

    #[test]
    fn test_system_affecting_apt_install() {
        let args = r#"{"command": "apt install libgtk-4-dev"}"#;
        assert!(
            is_system_affecting_command(args),
            "apt install should be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_sudo() {
        let args = r#"{"command": "sudo make install"}"#;
        assert!(
            is_system_affecting_command(args),
            "sudo should be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_npm_global() {
        let args = r#"{"command": "npm install -g typescript"}"#;
        assert!(
            is_system_affecting_command(args),
            "npm -g should be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_pip_install() {
        let args = r#"{"command": "pip install requests"}"#;
        assert!(
            is_system_affecting_command(args),
            "pip install without --user should be system-affecting"
        );
    }

    #[test]
    fn test_not_system_affecting_pip_user() {
        let args = r#"{"command": "pip install --user requests"}"#;
        assert!(
            !is_system_affecting_command(args),
            "pip install --user should not be system-affecting"
        );
    }

    #[test]
    fn test_not_system_affecting_cargo_build() {
        let args = r#"{"command": "cargo build"}"#;
        assert!(
            !is_system_affecting_command(args),
            "cargo build should not be system-affecting"
        );
    }

    #[test]
    fn test_not_system_affecting_ls() {
        let args = r#"{"command": "ls -la"}"#;
        assert!(
            !is_system_affecting_command(args),
            "ls should not be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_systemctl() {
        let args = r#"{"command": "systemctl restart nginx"}"#;
        assert!(
            is_system_affecting_command(args),
            "systemctl should be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_gem_install() {
        let args = r#"{"command": "gem install bundler"}"#;
        assert!(
            is_system_affecting_command(args),
            "gem install should be system-affecting"
        );
    }

    #[test]
    fn test_system_affecting_not_auto_approved() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let settings = test_settings(true, true, true);
        let args = r#"{"command": "apt install foo"}"#;

        assert!(
            !should_auto_approve(
                "execute_command",
                ToolCategory::ExecuteCommand,
                args,
                dir.path(),
                &settings
            ),
            "system-affecting command should not be auto-approved"
        );
    }
}
