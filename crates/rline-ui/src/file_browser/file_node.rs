//! FileNode — GObject subclass representing a file or directory in the tree.

use std::cell::RefCell;

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::Properties;

mod imp {
    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::FileNode)]
    pub struct FileNode {
        #[property(get, set)]
        name: RefCell<String>,
        #[property(get, set)]
        path: RefCell<String>,
        #[property(get, set, name = "is-directory")]
        is_directory: RefCell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileNode {
        const NAME: &'static str = "RlineFileNode";
        type Type = super::FileNode;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for FileNode {}
}

glib::wrapper! {
    /// A file or directory node for the file browser tree.
    pub struct FileNode(ObjectSubclass<imp::FileNode>);
}

impl FileNode {
    /// Create a new file node.
    pub fn new(name: &str, path: &str, is_directory: bool) -> Self {
        glib::Object::builder()
            .property("name", name)
            .property("path", path)
            .property("is-directory", is_directory)
            .build()
    }
}
