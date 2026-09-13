//! Reads what the viewer draws from an open library.

use std::path::PathBuf;

use anyhow::Result;
use epubsync_core::library::{Book, Library, ProgressRow};

/// One book in the list: its row and the path of its file.
#[derive(Debug, Clone)]
pub struct Entry {
    pub book: Book,
    pub path: PathBuf,
}

/// The books in id order, each with its file path, and every progress
/// row.
pub fn lists(lib: &Library) -> Result<(Vec<Entry>, Vec<ProgressRow>)> {
    let entries = lib
        .list()?
        .into_iter()
        .map(|book| Entry {
            path: lib.book_path(book.id),
            book,
        })
        .collect();
    Ok((entries, lib.progress()?))
}
