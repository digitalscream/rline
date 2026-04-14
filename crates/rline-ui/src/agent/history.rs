//! Persistence and discovery of saved agent conversations.
//!
//! Each completed agent run is written to `<workspace>/.agent-history/` as a
//! pair of files with a shared timestamp stem:
//!
//! - `<stem>.md` — human-readable Markdown transcript (existed before this
//!   module and is still the primary on-disk artifact users browse in a file
//!   manager).
//! - `<stem>.json` — full [`ConversationContext`] serialisation used to
//!   resume the conversation from the agent panel.
//!
//! Older sessions that only have a `.md` file surface in [`list_history`]
//! with `resumable == false` so the UI can mark them as read-only.
//!
//! I/O failures degrade gracefully: a missing directory yields an empty list,
//! malformed JSON demotes the entry to read-only, and deletion of the two
//! sidecars is best-effort per file.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rline_ai::agent::context::ConversationContext;

/// Maximum characters of the first user message to show as a preview.
const PREVIEW_MAX_CHARS: usize = 120;

/// A discovered conversation history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The filename stem (e.g. `2026-04-14_15-30-02`). Shared by both sidecars.
    pub stem: String,
    /// Modification time of the Markdown file, used for sorting.
    pub timestamp: SystemTime,
    /// A short preview derived from the first user message.
    pub preview: String,
    /// Whether the JSON sidecar exists and can be loaded into a context.
    pub resumable: bool,
    /// Path to the JSON sidecar, if present.
    pub json_path: Option<PathBuf>,
    /// Path to the Markdown transcript (always present).
    pub md_path: PathBuf,
}

/// Scan `<workspace>/.agent-history/` and return all entries newest-first.
///
/// Returns an empty list when the directory does not exist or cannot be read.
pub fn list_history(workspace_root: &Path) -> Vec<HistoryEntry> {
    let dir = workspace_root.join(".agent-history");
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    // Group files by stem, collecting the md + optional json path.
    let mut by_stem: BTreeMap<String, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();

    for entry in read.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let slot = by_stem.entry(stem).or_insert((None, None));
        match ext {
            "md" => slot.0 = Some(path),
            "json" => slot.1 = Some(path),
            _ => {}
        }
    }

    let mut entries: Vec<HistoryEntry> = by_stem
        .into_iter()
        .filter_map(|(stem, (md, json))| md.map(|md_path| (stem, md_path, json)))
        .map(|(stem, md_path, json_path)| {
            let timestamp = md_path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let preview = build_preview(json_path.as_deref(), &md_path);
            HistoryEntry {
                stem,
                timestamp,
                preview,
                resumable: json_path.is_some(),
                json_path,
                md_path,
            }
        })
        .collect();

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries
}

/// Load a `HistoryEntry` back into a resumable [`ConversationContext`].
pub fn load_context(entry: &HistoryEntry) -> io::Result<ConversationContext> {
    let json_path = entry.json_path.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "conversation has no JSON sidecar and cannot be resumed",
        )
    })?;
    let json = fs::read_to_string(json_path)?;
    ConversationContext::from_json(&json).map_err(io::Error::other)
}

/// Delete both sidecars for an entry. Errors on the JSON deletion are
/// ignored (it may not exist); the Markdown deletion is the authoritative
/// result.
pub fn delete_entry(entry: &HistoryEntry) -> io::Result<()> {
    if let Some(json) = &entry.json_path {
        let _ = fs::remove_file(json);
    }
    fs::remove_file(&entry.md_path)
}

/// Extract a one-line preview for the list row.
///
/// Prefers the JSON (first user message) because it is unambiguous; falls
/// back to scraping the Markdown `## User` section.
fn build_preview(json_path: Option<&Path>, md_path: &Path) -> String {
    if let Some(json_path) = json_path {
        if let Ok(json) = fs::read_to_string(json_path) {
            if let Ok(ctx) = ConversationContext::from_json(&json) {
                if let Some(first) = ctx.messages().iter().find(|m| {
                    matches!(m.role, rline_ai::chat::types::Role::User) && m.content.is_some()
                }) {
                    if let Some(content) = &first.content {
                        return truncate_preview(&content.as_text());
                    }
                }
            }
        }
    }

    if let Ok(md) = fs::read_to_string(md_path) {
        if let Some(text) = first_user_from_markdown(&md) {
            return truncate_preview(&text);
        }
    }

    String::from("(empty conversation)")
}

/// Scrape the first block under a `## User` heading from a Markdown
/// transcript produced by `ConversationContext::to_markdown`.
fn first_user_from_markdown(md: &str) -> Option<String> {
    let mut lines = md.lines();
    while let Some(line) = lines.next() {
        if line.trim_start() == "## User" {
            let mut buf = String::new();
            for body in lines.by_ref() {
                if body.starts_with("## ") {
                    break;
                }
                if !body.is_empty() {
                    if !buf.is_empty() {
                        buf.push(' ');
                    }
                    buf.push_str(body.trim());
                }
            }
            if !buf.is_empty() {
                return Some(buf);
            }
        }
    }
    None
}

fn truncate_preview(text: &str) -> String {
    let collapsed: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = collapsed.trim();
    if trimmed.chars().count() <= PREVIEW_MAX_CHARS {
        return trimmed.to_owned();
    }
    let mut out: String = trimmed.chars().take(PREVIEW_MAX_CHARS).collect();
    out.push('…');
    out
}

/// Format an entry's stem for display. The filename already encodes the
/// timestamp; we only need to make it a bit more readable.
pub fn format_stem(stem: &str) -> String {
    // stem shape: `YYYY-MM-DD_HH-MM-SS`
    match stem.split_once('_') {
        Some((date, time)) => format!("{date} {}", time.replace('-', ":")),
        None => stem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn list_history_returns_empty_when_dir_missing() {
        let tmp = tempdir().unwrap();
        assert!(list_history(tmp.path()).is_empty());
    }

    #[test]
    fn list_history_pairs_md_and_json_sidecars() {
        let tmp = tempdir().unwrap();
        let hist = tmp.path().join(".agent-history");
        let md = hist.join("2026-04-14_10-00-00.md");
        let json = hist.join("2026-04-14_10-00-00.json");

        let mut ctx = ConversationContext::new("sys", 100_000);
        ctx.add_user_message("hello there friend");
        ctx.add_assistant_message("hi");

        write(&md, &ctx.to_markdown());
        write(&json, &ctx.to_json().unwrap());

        let entries = list_history(tmp.path());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].resumable);
        assert!(entries[0].preview.contains("hello there"));
    }

    #[test]
    fn list_history_marks_md_only_as_read_only() {
        let tmp = tempdir().unwrap();
        let hist = tmp.path().join(".agent-history");
        let md = hist.join("2025-01-01_09-00-00.md");
        write(
            &md,
            "# Agent Conversation\n\n## User\n\nold format message\n",
        );

        let entries = list_history(tmp.path());
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].resumable);
        assert!(entries[0].preview.contains("old format"));
    }

    #[test]
    fn load_context_round_trips() {
        let tmp = tempdir().unwrap();
        let hist = tmp.path().join(".agent-history");
        fs::create_dir_all(&hist).unwrap();
        let md = hist.join("2026-04-14_11-00-00.md");
        let json = hist.join("2026-04-14_11-00-00.json");

        let mut ctx = ConversationContext::new("sys", 100_000);
        ctx.add_user_message("resume me");
        write(&md, &ctx.to_markdown());
        write(&json, &ctx.to_json().unwrap());

        let entries = list_history(tmp.path());
        let loaded = load_context(&entries[0]).unwrap();
        assert_eq!(loaded.message_count(), 1);
    }

    #[test]
    fn delete_entry_removes_both_sidecars() {
        let tmp = tempdir().unwrap();
        let hist = tmp.path().join(".agent-history");
        fs::create_dir_all(&hist).unwrap();
        let md = hist.join("2026-04-14_12-00-00.md");
        let json = hist.join("2026-04-14_12-00-00.json");
        write(&md, "transcript");
        write(&json, "{}");

        let entries = list_history(tmp.path());
        delete_entry(&entries[0]).unwrap();
        assert!(!md.exists());
        assert!(!json.exists());
    }

    #[test]
    fn format_stem_normalises_timestamp() {
        assert_eq!(format_stem("2026-04-14_10-00-00"), "2026-04-14 10:00:00");
        assert_eq!(format_stem("fallback"), "fallback");
    }

    #[test]
    fn truncate_preview_caps_long_text_and_collapses_newlines() {
        let long = "a".repeat(200);
        let out = truncate_preview(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), PREVIEW_MAX_CHARS + 1);

        assert_eq!(truncate_preview("line one\nline two"), "line one line two");
    }
}
