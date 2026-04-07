//! Background git operations — status, diff, staging, and discard.
//!
//! All functions are synchronous and designed to run on background threads.
//! Each function opens a fresh `git2::Repository` handle, so they are safe
//! to call from `std::thread::spawn`.

use std::path::{Path, PathBuf};

/// The kind of change detected for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// File content was modified.
    Modified,
    /// File is newly added (untracked or staged new).
    Added,
    /// File was deleted.
    Deleted,
    /// File was renamed.
    Renamed,
    /// File type changed (e.g. symlink ↔ regular file).
    Typechange,
    /// File has merge conflicts.
    Conflicted,
}

impl FileStatus {
    /// Short single-character label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Typechange => "T",
            Self::Conflicted => "C",
        }
    }
}

/// Status information for a single file.
#[derive(Debug, Clone)]
pub struct GitFileStatus {
    /// Path relative to the repository root.
    pub path: PathBuf,
    /// The kind of change.
    pub status: FileStatus,
}

/// Result of a full repository status query.
#[derive(Debug, Clone, Default)]
pub struct GitStatusResult {
    /// Files with staged (index) changes.
    pub staged: Vec<GitFileStatus>,
    /// Files with unstaged (working tree) changes.
    pub unstaged: Vec<GitFileStatus>,
    /// Current branch name, if on a branch.
    pub branch_name: Option<String>,
}

/// A single diff hunk with line ranges.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Starting line in the old file (1-based).
    pub old_start: u32,
    /// Number of lines in the old side.
    pub old_lines: u32,
    /// Starting line in the new file (1-based).
    pub new_start: u32,
    /// Number of lines in the new side.
    pub new_lines: u32,
}

/// Diff information for a single file, including full content of both sides.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Content from HEAD or index (the "old" side).
    pub old_content: String,
    /// Content from the working tree or index (the "new" side).
    pub new_content: String,
    /// Hunk ranges describing where changes occur.
    pub hunks: Vec<DiffHunk>,
}

/// Query the full status of a git repository.
///
/// Opens a fresh repository handle at `root` and returns classified staged
/// and unstaged changes along with the current branch name.
pub fn get_status(root: &Path) -> Result<GitStatusResult, git2::Error> {
    let repo = git2::Repository::discover(root)?;

    let branch_name = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(String::from));

    let statuses = repo.statuses(Some(
        git2::StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(true),
    ))?;

    let mut result = GitStatusResult {
        staged: Vec::new(),
        unstaged: Vec::new(),
        branch_name,
    };

    for entry in statuses.iter() {
        let path = match entry.path() {
            Some(p) => PathBuf::from(p),
            None => continue,
        };
        let st = entry.status();

        // Staged (index) changes
        if st.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        ) {
            let status = if st.contains(git2::Status::INDEX_NEW) {
                FileStatus::Added
            } else if st.contains(git2::Status::INDEX_MODIFIED) {
                FileStatus::Modified
            } else if st.contains(git2::Status::INDEX_DELETED) {
                FileStatus::Deleted
            } else if st.contains(git2::Status::INDEX_RENAMED) {
                FileStatus::Renamed
            } else {
                FileStatus::Typechange
            };
            result.staged.push(GitFileStatus {
                path: path.clone(),
                status,
            });
        }

        // Unstaged (working tree) changes
        if st.intersects(
            git2::Status::WT_NEW
                | git2::Status::WT_MODIFIED
                | git2::Status::WT_DELETED
                | git2::Status::WT_RENAMED
                | git2::Status::WT_TYPECHANGE
                | git2::Status::CONFLICTED,
        ) {
            let status = if st.contains(git2::Status::CONFLICTED) {
                FileStatus::Conflicted
            } else if st.contains(git2::Status::WT_NEW) {
                FileStatus::Added
            } else if st.contains(git2::Status::WT_MODIFIED) {
                FileStatus::Modified
            } else if st.contains(git2::Status::WT_DELETED) {
                FileStatus::Deleted
            } else if st.contains(git2::Status::WT_RENAMED) {
                FileStatus::Renamed
            } else {
                FileStatus::Typechange
            };
            result.unstaged.push(GitFileStatus { path, status });
        }
    }

    // Sort both lists by path for stable display.
    result.staged.sort_by(|a, b| a.path.cmp(&b.path));
    result.unstaged.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(result)
}

/// Get the diff for a single file.
///
/// When `staged` is true, diffs HEAD against the index (staged changes).
/// When `staged` is false, diffs the index against the working directory.
pub fn get_file_diff(root: &Path, file_path: &Path, staged: bool) -> Result<FileDiff, git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository has no working directory"))?;

    let relative = file_path.strip_prefix(workdir).unwrap_or(file_path);

    if staged {
        diff_staged(&repo, relative)
    } else {
        diff_unstaged(&repo, relative, workdir)
    }
}

/// Diff HEAD vs index for a single file (staged changes).
fn diff_staged(repo: &git2::Repository, relative: &Path) -> Result<FileDiff, git2::Error> {
    let head_tree = repo.head()?.peel_to_tree()?;

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(relative.to_string_lossy().as_ref());

    let diff = repo.diff_tree_to_index(Some(&head_tree), None, Some(&mut diff_opts))?;

    // Get old content from HEAD tree.
    let old_content = get_blob_content_from_tree(repo, &head_tree, relative);

    // Get new content from the index.
    let new_content = get_blob_content_from_index(repo, relative);

    let hunks = collect_hunks(&diff)?;

    Ok(FileDiff {
        old_content,
        new_content,
        hunks,
    })
}

/// Diff index vs working directory for a single file (unstaged changes).
fn diff_unstaged(
    repo: &git2::Repository,
    relative: &Path,
    workdir: &Path,
) -> Result<FileDiff, git2::Error> {
    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(relative.to_string_lossy().as_ref());

    let diff = repo.diff_index_to_workdir(None, Some(&mut diff_opts))?;

    // Old content from the index.
    let old_content = get_blob_content_from_index(repo, relative);

    // New content from the working directory.
    let abs_path = workdir.join(relative);
    let new_content = std::fs::read_to_string(&abs_path).unwrap_or_default();

    let hunks = collect_hunks(&diff)?;

    Ok(FileDiff {
        old_content,
        new_content,
        hunks,
    })
}

/// Extract hunk ranges from a git2 diff.
fn collect_hunks(diff: &git2::Diff<'_>) -> Result<Vec<DiffHunk>, git2::Error> {
    let mut hunks = Vec::new();
    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        Some(&mut |_delta, hunk| {
            hunks.push(DiffHunk {
                old_start: hunk.old_start(),
                old_lines: hunk.old_lines(),
                new_start: hunk.new_start(),
                new_lines: hunk.new_lines(),
            });
            true
        }),
        None,
    )?;
    Ok(hunks)
}

/// Read a blob's content from a tree entry.
fn get_blob_content_from_tree(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    path: &Path,
) -> String {
    tree.get_path(path)
        .ok()
        .and_then(|entry| entry.to_object(repo).ok())
        .and_then(|obj| obj.peel_to_blob().ok())
        .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
        .unwrap_or_default()
}

/// Read a blob's content from the index.
fn get_blob_content_from_index(repo: &git2::Repository, path: &Path) -> String {
    let index = match repo.index() {
        Ok(idx) => idx,
        Err(_) => return String::new(),
    };
    let path_bytes = path.to_string_lossy();
    index
        .get_path(Path::new(path_bytes.as_ref()), 0)
        .and_then(|entry| repo.find_blob(entry.id).ok())
        .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
        .unwrap_or_default()
}

/// Stage a file by adding it to the index.
///
/// For new or modified files, adds the working-tree content to the index.
/// For deleted files, removes the entry from the index.
pub fn stage_file(root: &Path, file_path: &Path) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository"))?;

    let relative = file_path.strip_prefix(workdir).unwrap_or(file_path);

    let mut index = repo.index()?;

    let abs_path = workdir.join(relative);
    if abs_path.exists() {
        index.add_path(relative)?;
    } else {
        index.remove_path(relative)?;
    }
    index.write()?;

    Ok(())
}

/// Unstage a file by resetting its index entry to match HEAD.
///
/// If the file does not exist in HEAD (i.e. it was newly added), the entry
/// is removed from the index entirely.
pub fn unstage_file(root: &Path, file_path: &Path) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository"))?;

    let relative = file_path.strip_prefix(workdir).unwrap_or(file_path);

    let head = repo.head()?.peel_to_tree()?;

    match head.get_path(relative) {
        Ok(entry) => {
            // File exists in HEAD — restore the index entry to the HEAD version.
            let mut index = repo.index()?;
            let blob = repo.find_blob(entry.id())?;
            let mut idx_entry = git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: entry.filemode() as u32,
                uid: 0,
                gid: 0,
                file_size: blob.content().len() as u32,
                id: entry.id(),
                flags: 0,
                flags_extended: 0,
                path: relative.to_string_lossy().as_bytes().to_vec(),
            };
            index.add_frombuffer(&idx_entry, blob.content())?;
            // Clear any flags that may have been set incorrectly.
            idx_entry.flags = 0;
            index.write()?;
        }
        Err(_) => {
            // File is new (not in HEAD) — remove from index to unstage.
            let mut index = repo.index()?;
            index.remove_path(relative)?;
            index.write()?;
        }
    }

    Ok(())
}

/// Discard working-tree changes to a file by checking out the index version.
///
/// For untracked files, this deletes the file from disk.
pub fn discard_file(root: &Path, file_path: &Path) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository"))?;

    let relative = file_path.strip_prefix(workdir).unwrap_or(file_path);

    // Check if the file is untracked (not in index).
    let index = repo.index()?;
    let in_index = index
        .get_path(Path::new(&*relative.to_string_lossy()), 0)
        .is_some();

    if !in_index {
        // Untracked file — just delete it.
        let abs_path = workdir.join(relative);
        if abs_path.exists() {
            std::fs::remove_file(&abs_path).map_err(|e| {
                git2::Error::from_str(&format!("failed to delete untracked file: {e}"))
            })?;
        }
        return Ok(());
    }

    // Tracked file — checkout from index.
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.path(relative.to_string_lossy().as_ref());
    checkout_opts.force();

    repo.checkout_index(None, Some(&mut checkout_opts))?;

    Ok(())
}

/// Stage all unstaged changes (equivalent to `git add -A`).
pub fn stage_all(root: &Path) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;

    // Also remove deleted files from the index.
    let status = repo.statuses(None)?;
    for entry in status.iter() {
        if entry.status().contains(git2::Status::WT_DELETED) {
            if let Some(path) = entry.path() {
                index.remove_path(Path::new(path))?;
            }
        }
    }

    index.write()?;
    Ok(())
}

/// Unstage all staged changes (equivalent to `git reset`).
pub fn unstage_all(root: &Path) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let head = repo.head()?.peel_to_commit()?;
    repo.reset(head.as_object(), git2::ResetType::Mixed, None)?;
    Ok(())
}

/// Summary of a git blame result for a single line.
#[derive(Debug, Clone)]
pub struct BlameInfo {
    /// Commit author name.
    pub author: String,
    /// Human-readable relative date (e.g. "2 days ago").
    pub date: String,
    /// First line of the commit message.
    pub summary: String,
}

/// Get blame information for a single line in a file.
///
/// `line` is 1-based. Returns `None`-equivalent via `Err` when the line
/// has no blame data (e.g. uncommitted new file).
pub fn get_blame_for_line(
    root: &Path,
    file_path: &Path,
    line: u32,
) -> Result<BlameInfo, git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository"))?;

    let relative = file_path.strip_prefix(workdir).unwrap_or(file_path);

    let mut opts = git2::BlameOptions::new();
    opts.min_line(line as usize);
    opts.max_line(line as usize);

    let blame = repo.blame_file(relative, Some(&mut opts))?;
    let hunk = blame
        .get_line(line as usize)
        .ok_or_else(|| git2::Error::from_str("no blame info for line"))?;

    let sig = hunk.final_signature();
    let author = sig.name().unwrap_or("Unknown").to_string();
    let epoch = sig.when().seconds();
    let date = format_relative_time(epoch);

    let commit = repo.find_commit(hunk.final_commit_id())?;
    let summary = commit.summary().unwrap_or("").to_string();

    Ok(BlameInfo {
        author,
        date,
        summary,
    })
}

/// Basic repository metadata for the status bar.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Repository name (last component of the workdir path).
    pub name: String,
    /// Current branch name, or HEAD commit short hash if detached.
    pub branch: String,
}

/// Get the repository name and current branch.
pub fn get_repo_info(root: &Path) -> Result<RepoInfo, git2::Error> {
    let repo = git2::Repository::discover(root)?;

    let name = repo
        .workdir()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let branch = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(String::from))
        .unwrap_or_else(|| "HEAD".to_string());

    Ok(RepoInfo { name, branch })
}

/// Format an epoch timestamp as a human-readable relative time string.
fn format_relative_time(epoch: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let diff = now - epoch;
    if diff < 0 {
        return "in the future".to_string();
    }

    let diff = diff as u64;
    match diff {
        0..=59 => "just now".to_string(),
        60..=3599 => {
            let m = diff / 60;
            if m == 1 {
                "1 minute ago".to_string()
            } else {
                format!("{m} minutes ago")
            }
        }
        3600..=86399 => {
            let h = diff / 3600;
            if h == 1 {
                "1 hour ago".to_string()
            } else {
                format!("{h} hours ago")
            }
        }
        86400..=2_591_999 => {
            let d = diff / 86400;
            if d == 1 {
                "1 day ago".to_string()
            } else {
                format!("{d} days ago")
            }
        }
        2_592_000..=31_535_999 => {
            let m = diff / 2_592_000;
            if m == 1 {
                "1 month ago".to_string()
            } else {
                format!("{m} months ago")
            }
        }
        _ => {
            let y = diff / 31_536_000;
            if y == 1 {
                "1 year ago".to_string()
            } else {
                format!("{y} years ago")
            }
        }
    }
}

/// Create a commit with the given message from the current index.
pub fn commit(root: &Path, message: &str) -> Result<(), git2::Error> {
    let repo = git2::Repository::discover(root)?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let sig = repo.signature()?;
    let head = repo.head()?.peel_to_commit()?;
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&head])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temporary git repo with an initial commit.
    fn setup_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path().to_path_buf();

        let repo = git2::Repository::init(&root).expect("init repo");

        // Configure user for commits.
        let mut config = repo.config().expect("get config");
        config.set_str("user.name", "Test").expect("set name");
        config
            .set_str("user.email", "test@test.com")
            .expect("set email");

        // Create initial file and commit.
        let file_path = root.join("hello.txt");
        fs::write(&file_path, "hello world\n").expect("write file");

        let mut index = repo.index().expect("get index");
        index
            .add_path(Path::new("hello.txt"))
            .expect("add to index");
        index.write().expect("write index");

        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = repo.signature().expect("signature");
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .expect("commit");

        (dir, root)
    }

    #[test]
    fn test_get_status_clean_repo() {
        let (_dir, root) = setup_test_repo();
        let result = get_status(&root).expect("get status");
        assert!(result.staged.is_empty(), "no staged changes");
        assert!(result.unstaged.is_empty(), "no unstaged changes");
    }

    #[test]
    fn test_get_status_modified_file() {
        let (_dir, root) = setup_test_repo();

        // Modify the file.
        fs::write(root.join("hello.txt"), "modified\n").expect("write");

        let result = get_status(&root).expect("get status");
        assert!(result.staged.is_empty(), "nothing staged");
        assert_eq!(result.unstaged.len(), 1, "one unstaged change");
        assert_eq!(result.unstaged[0].status, FileStatus::Modified);
    }

    #[test]
    fn test_stage_and_unstage_file() {
        let (_dir, root) = setup_test_repo();
        let file = root.join("hello.txt");

        fs::write(&file, "modified\n").expect("write");

        // Stage it.
        stage_file(&root, &file).expect("stage");
        let result = get_status(&root).expect("status after stage");
        assert_eq!(result.staged.len(), 1, "one staged");
        assert!(result.unstaged.is_empty(), "nothing unstaged");

        // Unstage it.
        unstage_file(&root, &file).expect("unstage");
        let result = get_status(&root).expect("status after unstage");
        assert!(result.staged.is_empty(), "nothing staged");
        assert_eq!(result.unstaged.len(), 1, "back to unstaged");
    }

    #[test]
    fn test_discard_file() {
        let (_dir, root) = setup_test_repo();
        let file = root.join("hello.txt");

        fs::write(&file, "modified\n").expect("write");

        discard_file(&root, &file).expect("discard");
        let content = fs::read_to_string(&file).expect("read");
        assert_eq!(content, "hello world\n", "content restored");
    }

    #[test]
    fn test_get_file_diff_unstaged() {
        let (_dir, root) = setup_test_repo();
        let file = root.join("hello.txt");

        fs::write(&file, "modified content\n").expect("write");

        let diff = get_file_diff(&root, &file, false).expect("diff");
        assert_eq!(diff.old_content, "hello world\n");
        assert_eq!(diff.new_content, "modified content\n");
        assert!(!diff.hunks.is_empty(), "should have at least one hunk");
    }
}
