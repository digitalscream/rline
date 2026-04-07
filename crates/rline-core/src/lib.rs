//! rline-core — Text buffer domain types, document model, and position primitives.

pub mod document;
pub mod error;
pub mod position;
pub mod search;

pub use document::{DocumentId, DocumentMeta};
pub use error::CoreError;
pub use position::{ByteOffset, CharOffset, LineIndex};
pub use search::SearchResult;
