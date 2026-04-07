//! Document identity and metadata types.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique identifier for an open document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl DocumentId {
    /// Generate a new unique document ID.
    pub fn next() -> Self {
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Metadata about an open document.
#[derive(Debug)]
pub struct DocumentMeta {
    id: DocumentId,
    path: Option<PathBuf>,
    language_id: Option<String>,
}

impl DocumentMeta {
    /// Create metadata for a new document backed by a file.
    pub fn from_path(path: PathBuf) -> Self {
        let language_id = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_owned());
        Self {
            id: DocumentId::next(),
            path: Some(path),
            language_id,
        }
    }

    /// Create metadata for an untitled document.
    pub fn untitled() -> Self {
        Self {
            id: DocumentId::next(),
            path: None,
            language_id: None,
        }
    }

    /// The unique identifier for this document.
    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// The file path, if this document is backed by a file.
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// The language identifier derived from the file extension.
    pub fn language_id(&self) -> Option<&str> {
        self.language_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_id_next_generates_unique_ids() {
        let id1 = DocumentId::next();
        let id2 = DocumentId::next();
        let id3 = DocumentId::next();
        assert_ne!(id1, id2, "consecutive IDs should be different");
        assert_ne!(id2, id3, "consecutive IDs should be different");
        assert_ne!(id1, id3, "non-consecutive IDs should be different");
    }

    #[test]
    fn test_document_meta_from_path_extracts_language_id() {
        let meta = DocumentMeta::from_path(PathBuf::from("/tmp/hello.rs"));
        assert_eq!(
            meta.language_id(),
            Some("rs"),
            "language_id should be the file extension"
        );
    }

    #[test]
    fn test_document_meta_from_path_different_extension() {
        let meta = DocumentMeta::from_path(PathBuf::from("/home/user/notes.txt"));
        assert_eq!(
            meta.language_id(),
            Some("txt"),
            "language_id should match the extension"
        );
    }

    #[test]
    fn test_document_meta_from_path_no_extension() {
        let meta = DocumentMeta::from_path(PathBuf::from("/usr/bin/bash"));
        assert_eq!(
            meta.language_id(),
            None,
            "file without extension should have no language_id"
        );
    }

    #[test]
    fn test_document_meta_from_path_stores_path() {
        let path = PathBuf::from("/tmp/test.py");
        let meta = DocumentMeta::from_path(path.clone());
        assert_eq!(
            meta.path(),
            Some(&path),
            "path should be stored in metadata"
        );
    }

    #[test]
    fn test_document_meta_untitled_has_no_path() {
        let meta = DocumentMeta::untitled();
        assert_eq!(meta.path(), None, "untitled document should have no path");
    }

    #[test]
    fn test_document_meta_untitled_has_no_language_id() {
        let meta = DocumentMeta::untitled();
        assert_eq!(
            meta.language_id(),
            None,
            "untitled document should have no language_id"
        );
    }

    #[test]
    fn test_document_meta_untitled_has_unique_id() {
        let meta1 = DocumentMeta::untitled();
        let meta2 = DocumentMeta::untitled();
        assert_ne!(
            meta1.id(),
            meta2.id(),
            "each untitled document should get a unique id"
        );
    }
}
