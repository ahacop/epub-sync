//! The device layer: the trait a device type implements, and the plan
//! that compares the device with the library.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::metadata::Metadata;

/// One thing sync does to the device folder. As JSON it is one object
/// with the variant name in `action`, as `{"action": "send_again", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
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

/// How far the reader is through a book, as the Kobo classes it. The
/// Kobo stores it as 0, 1, or 2, and so does the library database. As
/// JSON it is the lowercase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadStatus {
    Unread,
    Reading,
    Finished,
}

impl ReadStatus {
    /// The stored value. A value other than 1 or 2 reads as unread.
    pub fn from_i64(n: i64) -> ReadStatus {
        match n {
            1 => ReadStatus::Reading,
            2 => ReadStatus::Finished,
            _ => ReadStatus::Unread,
        }
    }

    pub fn as_i64(self) -> i64 {
        match self {
            ReadStatus::Unread => 0,
            ReadStatus::Reading => 1,
            ReadStatus::Finished => 2,
        }
    }
}

impl rusqlite::types::FromSql for ReadStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(ReadStatus::from_i64(i64::column_result(value)?))
    }
}

impl rusqlite::ToSql for ReadStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_i64().into())
    }
}

/// Reading progress for one book, as read from the device.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub book_id: i64,
    pub percent: i64,
    pub status: ReadStatus,
    pub last_read: Option<String>,
    /// The reading time in seconds, when the device counts it.
    pub time_spent: Option<i64>,
}

/// One word looked up in the device dictionary in a library book.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub word: String,
    pub book_id: i64,
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
    /// The count of history rows sync added: the books whose progress
    /// differs from the last read. A device returns 0, and sync sets it.
    pub changed: usize,
}

/// A connected device. Detection is a function of each device type, since
/// it runs before there is a device.
pub trait Device {
    fn serial(&self) -> &str;
    /// The ids of the books in the device folder.
    fn list(&self) -> Result<BTreeSet<i64>>;
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
/// returns the actions in id order. Pure: no I/O. `books` and `sent` map
/// a book id to a revision.
pub fn plan(
    books: &BTreeMap<i64, i64>,
    on_device: &BTreeSet<i64>,
    sent: &BTreeMap<i64, i64>,
) -> Vec<Action> {
    let ids: BTreeSet<i64> = books.keys().chain(on_device).copied().collect();
    ids.into_iter()
        .filter_map(
            |id| match (books.get(&id), on_device.contains(&id), sent.get(&id)) {
                (None, _, _) => Some(Action::Delete { id }),
                (Some(&revision), false, None) => Some(Action::Send { id, revision }),
                (Some(&revision), false, Some(_)) => Some(Action::SendAgain { id, revision }),
                (Some(revision), true, Some(sent)) if sent == revision => None,
                (Some(&revision), true, _) => Some(Action::Replace { id, revision }),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revisions(pairs: &[(i64, i64)]) -> BTreeMap<i64, i64> {
        pairs.iter().copied().collect()
    }

    fn ids(ids: &[i64]) -> BTreeSet<i64> {
        ids.iter().copied().collect()
    }

    #[test]
    fn sends_a_new_book() {
        assert_eq!(
            plan(&revisions(&[(1, 1)]), &ids(&[]), &revisions(&[])),
            vec![Action::Send { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn does_nothing_for_a_book_at_the_sent_revision() {
        assert_eq!(
            plan(&revisions(&[(1, 2)]), &ids(&[1]), &revisions(&[(1, 2)])),
            vec![]
        );
    }

    #[test]
    fn replaces_a_book_whose_revision_changed() {
        assert_eq!(
            plan(&revisions(&[(1, 3)]), &ids(&[1]), &revisions(&[(1, 2)])),
            vec![Action::Replace { id: 1, revision: 3 }]
        );
    }

    #[test]
    fn replaces_a_device_file_with_no_sent_row() {
        assert_eq!(
            plan(&revisions(&[(1, 1)]), &ids(&[1]), &revisions(&[])),
            vec![Action::Replace { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn sends_again_a_book_deleted_on_the_device() {
        assert_eq!(
            plan(&revisions(&[(1, 1)]), &ids(&[]), &revisions(&[(1, 1)])),
            vec![Action::SendAgain { id: 1, revision: 1 }]
        );
    }

    #[test]
    fn deletes_a_device_file_with_no_book() {
        assert_eq!(
            plan(&revisions(&[]), &ids(&[7]), &revisions(&[(7, 1)])),
            vec![Action::Delete { id: 7 }]
        );
    }

    #[test]
    fn orders_by_id() {
        let actions = plan(
            &revisions(&[(1, 1), (2, 2), (3, 1)]),
            &ids(&[2, 3, 9]),
            &revisions(&[(2, 1), (3, 1)]),
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
