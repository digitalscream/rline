//! OpenBufferItem — GObject subclass representing an open editor buffer in the list.

use std::cell::RefCell;

use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::Properties;

mod imp {
    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::OpenBufferItem)]
    pub struct OpenBufferItem {
        #[property(get, set)]
        name: RefCell<String>,
        #[property(get, set)]
        path: RefCell<String>,
        #[property(get, set, name = "is-modified")]
        is_modified: RefCell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OpenBufferItem {
        const NAME: &'static str = "RlineOpenBufferItem";
        type Type = super::OpenBufferItem;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for OpenBufferItem {}
}

glib::wrapper! {
    /// An open editor buffer entry for the file browser's buffer list.
    pub struct OpenBufferItem(ObjectSubclass<imp::OpenBufferItem>);
}

impl OpenBufferItem {
    /// Create a new open buffer item.
    pub fn new(name: &str, path: &str, is_modified: bool) -> Self {
        glib::Object::builder()
            .property("name", name)
            .property("path", path)
            .property("is-modified", is_modified)
            .build()
    }
}
