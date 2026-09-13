//! The device layer: the trait a device type implements, and the plan
//! that compares the device with the library.

use std::path::Path;

use anyhow::Result;

use crate::metadata::Metadata;

/// One thing sync does to the device folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The book has no file on the device and was never sent.
    Send { id: i64, revision: i64 },
    /// The book's revision changed since the last send.
    Replace { id: i64, revision: i64 },
    /// The book was sent before, but its file is gone from the device.
    SendAgain { id: i64, revision: i64 },
    /// The device file matches no library book.
    Delete { id: i64 },
}

impl Action {
    pub fn id(&self) -> i64 {
        match self {
            Action::Send { id, .. }
            | Action::Replace { id, .. }
            | Action::SendAgain { id, .. }
            | Action::Delete { id } => *id,
        }
    }
}

/// A library book as the plan sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookRevision {
    pub id: i64,
    pub revision: i64,
}

/// Reading progress for one book, as read from the device.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub book_id: i64,
    pub percent: i64,
    pub status: i64,
    pub last_read: Option<String>,
}

/// One word looked up in the device dictionary.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub word: String,
    /// The library book id when the word came from a library book.
    pub book_id: Option<i64>,
    pub volume_id: String,
    pub book_title: Option<String>,
    pub dict_suffix: Option<String>,
    pub looked_up_at: String,
}

/// What an update of a book's row on the device did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowUpdate {
    /// One or more columns differed and were written.
    Updated,
    /// The row already matched the record.
    Unchanged,
    /// The device has not created the row yet.
    NoRow,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadBack {
    pub progress: Vec<Progress>,
    pub words: Vec<Word>,
}

/// A connected device. Detection is a function of each device type, since
/// it runs before there is a device.
pub trait Device {
    fn serial(&self) -> &str;
    /// The ids of the books in the device folder.
    fn list(&self) -> Result<Vec<i64>>;
    /// Runs one action. `source` is the library file for a send or a
    /// replace, and unused for a delete.
    fn apply(&mut self, action: &Action, source: &Path) -> Result<()>;
    /// Why the device refuses row writes, when it does. Sync then skips
    /// replacements too, because a replacement without its row update
    /// makes the firmware treat the file as a new book.
    fn write_gate(&self) -> Option<String>;
    /// Keeps each book's device row equal to its record, in one
    /// transaction. Returns what happened per book.
    fn update_rows(&mut self, records: &[(i64, &Metadata)]) -> Result<Vec<(i64, RowUpdate)>>;
    /// Reads progress for the given book ids and every looked-up word.
    fn read_back(&mut self, book_ids: &[i64]) -> Result<ReadBack>;
    /// Flushes and closes anything sync opened on the device.
    fn finish(&mut self) -> Result<()>;
}

/// Compares the library, the device folder, and the `sent` table, and
/// returns the actions in id order. Pure: no I/O.
pub fn plan(books: &[BookRevision], on_device: &[i64], sent: &[BookRevision]) -> Vec<Action> {
    let mut actions = Vec::new();
    for book in books {
        let present = on_device.contains(&book.id);
        let sent_revision = sent.iter().find(|s| s.id == book.id).map(|s| s.revision);
        match (present, sent_revision) {
            (false, None) => actions.push(Action::Send {
                id: book.id,
                revision: book.revision,
            }),
            (false, Some(_)) => actions.push(Action::SendAgain {
                id: book.id,
                revision: book.revision,
            }),
            (true, Some(r)) if r == book.revision => {}
            (true, _) => actions.push(Action::Replace {
                id: book.id,
                revision: book.revision,
            }),
        }
    }
    for id in on_device {
        if !books.iter().any(|b| b.id == *id) {
            actions.push(Action::Delete { id: *id });
        }
    }
    actions.sort_by_key(Action::id);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: i64, revision: i64) -> BookRevision {
        BookRevision { id, revision }
    }

    #[test]
    fn sends_a_new_book() {
        assert_eq!(
            plan(&[b(1, 1)], &[], &[]),
            vec![Action::Send { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn does_nothing_for_a_book_at_the_sent_revision() {
        assert_eq!(plan(&[b(1, 2)], &[1], &[b(1, 2)]), vec![]);
    }

    #[test]
    fn replaces_a_book_whose_revision_changed() {
        assert_eq!(
            plan(&[b(1, 3)], &[1], &[b(1, 2)]),
            vec![Action::Replace { id: 1, revision: 3 }]
        );
    }

    #[test]
    fn replaces_a_device_file_with_no_sent_row() {
        assert_eq!(
            plan(&[b(1, 1)], &[1], &[]),
            vec![Action::Replace { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn sends_again_a_book_deleted_on_the_device() {
        assert_eq!(
            plan(&[b(1, 1)], &[], &[b(1, 1)]),
            vec![Action::SendAgain { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn deletes_a_device_file_with_no_book() {
        assert_eq!(plan(&[], &[7], &[b(7, 1)]), vec![Action::Delete { id: 7 }]);
    }

    #[test]
    fn orders_by_id() {
        let actions = plan(
            &[b(1, 1), b(2, 2), b(3, 1)],
            &[2, 3, 9],
            &[b(2, 1), b(3, 1)],
        );
        assert_eq!(
            actions,
            vec![
                Action::Send { id: 1, revision: 1 },
                Action::Replace { id: 2, revision: 2 },
                Action::Delete { id: 9 },
            ]
        );
    }
}
