//! Background search worker — walks the project tree and searches file contents.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use rline_core::position::LineIndex;
use rline_core::SearchResult;

/// Directories to skip during search.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".hg",
    "build",
    "dist",
    "__pycache__",
];

/// Search all files under `root` for lines containing `query`.
///
/// Results are sent via the `sender` channel. The search can be cancelled
/// by dropping the receiver.
pub fn search_files(root: &Path, query: &str, sender: &std::sync::mpsc::Sender<SearchResult>) {
    let query_lower = query.to_lowercase();
    walk_and_search(root, &query_lower, sender);
}

fn walk_and_search(dir: &Path, query: &str, sender: &std::sync::mpsc::Sender<SearchResult>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden files and directories
        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Skip known non-content directories
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            walk_and_search(&path, query, sender);
        } else if path.is_file() {
            search_file(&path, query, sender);
        }
    }
}

fn search_file(path: &Path, query: &str, sender: &std::sync::mpsc::Sender<SearchResult>) {
    // Skip binary files (simple heuristic: check extension)
    if is_likely_binary(path) {
        return;
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file);

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break, // binary or encoding error
        };

        let line_lower = line.to_lowercase();
        if let Some(pos) = line_lower.find(query) {
            let result = SearchResult {
                path: path.to_path_buf(),
                line_number: LineIndex(line_num),
                line_text: line.clone(),
                match_start: pos,
                match_end: pos + query.len(),
            };
            // If send fails, the receiver was dropped (search cancelled)
            if sender.send(result).is_err() {
                return;
            }
        }
    }
}

fn is_likely_binary(path: &Path) -> bool {
    const BINARY_EXTENSIONS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "ico", "svg", "pdf", "zip", "gz", "tar", "bz2", "xz",
        "7z", "exe", "dll", "so", "dylib", "o", "a", "wasm", "class", "pyc", "pyo", "ttf", "otf",
        "woff", "woff2", "eot", "mp3", "mp4", "avi", "mkv", "flac", "wav", "db", "sqlite",
        "sqlite3",
    ];

    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Collect all file paths under `root` for the quick-open index.
pub fn collect_file_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_paths_recursive(root, &mut paths, 0);
    paths
}

fn collect_paths_recursive(dir: &Path, paths: &mut Vec<PathBuf>, depth: usize) {
    // Cap depth and total count to avoid performance issues on huge projects
    if depth > 20 || paths.len() > 10_000 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            collect_paths_recursive(&path, paths, depth + 1);
        } else if path.is_file() {
            paths.push(path);
        }
    }
}
