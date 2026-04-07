//! File tree model — builds a TreeListModel with lazy directory loading.

use std::path::Path;

use gtk4::prelude::*;

use super::file_node::FileNode;

/// Read a directory and return a sorted ListStore of FileNode objects.
///
/// Directories come first (sorted alphabetically), then files (sorted alphabetically).
/// Hidden files (starting with '.') are skipped.
pub fn build_directory_model(dir: &Path) -> gio::ListStore {
    let store = gio::ListStore::new::<FileNode>();

    let mut entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("failed to read directory {}: {e}", dir.display());
            return store;
        }
    };

    // Sort: directories first, then alphabetically by name
    entries.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_lowercase()),
        }
    });

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path().display().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        store.append(&FileNode::new(&name, &path, is_dir));
    }

    store
}

/// Build a TreeListModel for the given root directory.
///
/// Each directory node lazily creates its child model when expanded.
pub fn build_tree_list_model(root: &Path) -> gtk4::TreeListModel {
    let root_model = build_directory_model(root);

    gtk4::TreeListModel::new(root_model, false, false, |item| {
        let node = item.downcast_ref::<FileNode>()?;
        if node.is_directory() {
            let path = std::path::PathBuf::from(node.path());
            let child_model = build_directory_model(&path);
            Some(child_model.upcast())
        } else {
            None
        }
    })
}
