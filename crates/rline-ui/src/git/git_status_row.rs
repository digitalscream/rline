//! GitStatusRow — GObject subclass representing a row in the git status list.
//!
//! Rows can be either section headers ("Staged Changes", "Changes") or
//! individual file entries with status information.

use std::cell::RefCell;

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::Properties;

mod imp {
    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::GitStatusRow)]
    pub struct GitStatusRow {
        /// Absolute or relative file path.
        #[property(get, set)]
        file_path: RefCell<String>,
        /// Display name (relative path for files, title for headers).
        #[property(get, set)]
        display_name: RefCell<String>,
        /// Status label ("M", "A", "D", "R", "T", "C") or empty for headers.
        #[property(get, set)]
        status: RefCell<String>,
        /// Whether the file is in the staged section.
        #[property(get, set, name = "is-staged")]
        is_staged: RefCell<bool>,
        /// Whether this row is a section header.
        #[property(get, set, name = "is-header")]
        is_header: RefCell<bool>,
        /// Number of items in this section (only meaningful for headers).
        #[property(get, set, name = "item-count")]
        item_count: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GitStatusRow {
        const NAME: &'static str = "RlineGitStatusRow";
        type Type = super::GitStatusRow;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for GitStatusRow {}
}

glib::wrapper! {
    /// A row in the git status panel — either a section header or a file entry.
    pub struct GitStatusRow(ObjectSubclass<imp::GitStatusRow>);
}

impl GitStatusRow {
    /// Create a section header row.
    pub fn new_header(title: &str, count: u32, is_staged: bool) -> Self {
        glib::Object::builder()
            .property("display-name", title)
            .property("is-header", true)
            .property("is-staged", is_staged)
            .property("item-count", count)
            .build()
    }

    /// Create a file entry row.
    pub fn new_file(file_path: &str, display_name: &str, status: &str, is_staged: bool) -> Self {
        glib::Object::builder()
            .property("file-path", file_path)
            .property("display-name", display_name)
            .property("status", status)
            .property("is-staged", is_staged)
            .property("is-header", false)
            .build()
    }
}
