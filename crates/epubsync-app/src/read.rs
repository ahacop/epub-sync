//! Reads what the viewer draws from an open library.

use anyhow::Result;
use epubsync_core::library::{Book, Library};

/// The books in id order.
pub fn books(lib: &Library) -> Result<Vec<Book>> {
    lib.list()
}
