//! Runs a sync against one device: plan, apply with the `sent` table
//! updated after each book, then read progress and words back. Progress
//! goes into `progress_history`: a book gets a new row when what the
//! device holds differs from the newest row, so the table is a log of
//! every change a sync saw.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use crate::device::{Action, Device, Progress, ReadBack, ReadStatus, RowUpdate};
use crate::library::{Library, PROGRESS_COLUMNS, ProgressRow, progress_from_row};

/// Records the device and returns the actions for it.
pub fn plan(library: &Library, device: &dyn Device) -> Result<Vec<Action>> {
    let serial = device.serial();
    library.db.execute(
        "INSERT OR IGNORE INTO devices (serial) VALUES (?1)",
        [serial],
    )?;
    let books: BTreeMap<i64, i64> = library.list()?.iter().map(|b| (b.id, b.revision)).collect();
    let mut stmt = library
        .db
        .prepare("SELECT book_id, revision FROM sent WHERE device_serial = ?1")?;
    let sent: BTreeMap<i64, i64> = stmt
        .query_map([serial], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let on_device = device.list()?;
    Ok(crate::device::plan(&books, &on_device, &sent))
}

/// Runs each action, then updates that book's `sent` row in one
/// transaction. Calls `progress` before each action.
pub fn apply(
    library: &mut Library,
    device: &mut dyn Device,
    actions: &[Action],
    mut progress: impl FnMut(&Action),
) -> Result<()> {
    let serial = device.serial().to_string();
    for action in actions {
        progress(action);
        let source = library.book_path(action.id());
        device
            .apply(action, &source)
            .with_context(|| format!("apply {action:?}"))?;
        let tx = library.db.transaction()?;
        match action {
            Action::Send { id, revision }
            | Action::Replace { id, revision }
            | Action::SendAgain { id, revision } => {
                tx.execute(
                    "INSERT INTO sent (book_id, device_serial, revision) VALUES (?1, ?2, ?3)
                     ON CONFLICT (book_id, device_serial) DO UPDATE SET revision = excluded.revision",
                    params![id, serial, revision],
                )?;
            }
            Action::Delete { id } => {
                tx.execute(
                    "DELETE FROM sent WHERE book_id = ?1 AND device_serial = ?2",
                    params![id, serial],
                )?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

/// Reads progress and words from the device and stores them. A book
/// whose progress differs from its newest history row gets a new row,
/// with `seen_at` set to the sync time. History rows go in one
/// transaction, word rows in another. A word the library already holds
/// is skipped.
pub fn read_back(library: &mut Library, device: &mut dyn Device) -> Result<ReadBack> {
    let serial = device.serial().to_string();
    let ids: Vec<i64> = library.list()?.iter().map(|b| b.id).collect();
    let back = device.read_back(&ids)?;

    let tx = library.db.transaction()?;
    // The sync time comes from SQLite so it has the format every other
    // timestamp the library writes has.
    let now: String = tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |r| {
        r.get(0)
    })?;
    let mut changed = 0;
    for p in &back.progress {
        let previous = tx
            .query_row(
                &format!(
                    "SELECT {PROGRESS_COLUMNS} FROM progress WHERE book_id = ?1 AND device_serial = ?2"
                ),
                params![p.book_id, serial],
                |r| progress_from_row(r, 0),
            )
            .optional()?;
        let Some(row) = next_row(previous.as_ref(), p, &now) else {
            continue;
        };
        tx.execute(
            "INSERT INTO progress_history (book_id, device_serial, percent, status, last_read, time_spent, finished_at, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                p.book_id,
                serial,
                row.percent,
                row.status,
                row.last_read,
                row.time_spent,
                row.finished_at,
                now
            ],
        )?;
        changed += 1;
    }
    tx.commit()?;

    let tx = library.db.transaction()?;
    let mut new_words = Vec::new();
    for w in &back.words {
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO words (word, device_serial, book_id, dict_suffix, looked_up_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![w.word, serial, w.book_id, w.dict_suffix, w.looked_up_at],
        )?;
        if inserted > 0 {
            new_words.push(w.clone());
        }
    }
    tx.commit()?;
    Ok(ReadBack {
        progress: back.progress,
        words: new_words,
        changed,
    })
}

/// The columns of a history row sync is about to add.
#[derive(Debug, Clone, PartialEq)]
struct NewRow {
    percent: i64,
    status: ReadStatus,
    last_read: Option<String>,
    time_spent: Option<i64>,
    finished_at: Option<String>,
}

/// The history row to add for what the device holds, or `None` when the
/// newest row already holds the same percent, status, last read time,
/// and reading time. Pure: no I/O.
///
/// `finished_at` is the read's last read time when the status turns
/// finished, or `now` when the device has no last read time. A status
/// that was finished before keeps the row's date, and a status that is
/// not finished carries the row's date forward.
fn next_row(previous: Option<&ProgressRow>, read: &Progress, now: &str) -> Option<NewRow> {
    if let Some(p) = previous
        && p.percent == read.percent
        && p.status == read.status
        && p.last_read == read.last_read
        && p.time_spent == read.time_spent
    {
        return None;
    }
    let turned_finished = read.status == ReadStatus::Finished
        && previous.is_none_or(|p| p.status != ReadStatus::Finished);
    let finished_at = if turned_finished {
        Some(read.last_read.clone().unwrap_or_else(|| now.to_string()))
    } else {
        previous.and_then(|p| p.finished_at.clone())
    };
    Some(NewRow {
        percent: read.percent,
        status: read.status,
        last_read: read.last_read.clone(),
        time_spent: read.time_spent,
        finished_at,
    })
}

/// The planned actions, split by the device's write gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Every action runs, and sync updates the rows after.
    Open(Vec<Action>),
    /// The device refuses row writes. Sync holds the replacements back.
    Closed {
        kept: Vec<Action>,
        skipped: Vec<Action>,
        reason: String,
    },
}

/// Splits the actions by the device's write gate: replacements are
/// skipped when the gate is up.
pub fn gate(actions: Vec<Action>, device: &dyn Device) -> Gate {
    match device.write_gate() {
        None => Gate::Open(actions),
        Some(reason) => {
            let (skipped, kept): (Vec<Action>, Vec<Action>) = actions
                .into_iter()
                .partition(|a| matches!(a, Action::Replace { .. }));
            Gate::Closed {
                kept,
                skipped,
                reason,
            }
        }
    }
}

/// Keeps every library book's device row equal to its record.
pub fn update_rows(library: &Library, device: &mut dyn Device) -> Result<Vec<(i64, RowUpdate)>> {
    let books = library.list()?;
    let records: Vec<(i64, &crate::metadata::Metadata)> =
        books.iter().map(|b| (b.id, &b.metadata)).collect();
    device.update_rows(&records)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-09-19T12:00:00Z";

    fn read(percent: i64, status: ReadStatus, last_read: Option<&str>) -> Progress {
        Progress {
            book_id: 1,
            percent,
            status,
            last_read: last_read.map(str::to_string),
            time_spent: Some(600),
        }
    }

    fn row(read: &Progress, finished_at: Option<&str>) -> ProgressRow {
        ProgressRow {
            device_serial: "N1".into(),
            percent: read.percent,
            status: read.status,
            last_read: read.last_read.clone(),
            time_spent: read.time_spent,
            finished_at: finished_at.map(str::to_string),
        }
    }

    #[test]
    fn the_first_read_makes_a_row() {
        let read = read(37, ReadStatus::Reading, Some("2026-09-01T10:00:00Z"));
        assert_eq!(
            next_row(None, &read, NOW),
            Some(NewRow {
                percent: 37,
                status: ReadStatus::Reading,
                last_read: Some("2026-09-01T10:00:00Z".into()),
                time_spent: Some(600),
                finished_at: None,
            })
        );
    }

    #[test]
    fn the_same_read_makes_no_row() {
        let read = read(37, ReadStatus::Reading, Some("2026-09-01T10:00:00Z"));
        assert_eq!(next_row(Some(&row(&read, None)), &read, NOW), None);
    }

    #[test]
    fn a_change_to_any_column_makes_a_row() {
        let read = read(37, ReadStatus::Reading, Some("2026-09-01T10:00:00Z"));
        let previous = row(&read, None);
        let changed = [
            Progress {
                percent: 38,
                ..read.clone()
            },
            Progress {
                status: ReadStatus::Unread,
                ..read.clone()
            },
            Progress {
                last_read: Some("2026-09-02T10:00:00Z".into()),
                ..read.clone()
            },
            Progress {
                time_spent: Some(601),
                ..read.clone()
            },
            Progress {
                time_spent: None,
                ..read.clone()
            },
        ];
        for read in &changed {
            assert!(next_row(Some(&previous), read, NOW).is_some(), "{read:?}");
        }
    }

    #[test]
    fn turning_finished_sets_the_date_from_the_last_read_time() {
        let reading = read(90, ReadStatus::Reading, Some("2026-09-01T10:00:00Z"));
        let finished = read(100, ReadStatus::Finished, Some("2026-09-03T10:00:00Z"));
        let row = next_row(Some(&row(&reading, None)), &finished, NOW).unwrap();
        assert_eq!(row.finished_at.as_deref(), Some("2026-09-03T10:00:00Z"));
        // The first read can be finished too.
        let row = next_row(None, &finished, NOW).unwrap();
        assert_eq!(row.finished_at.as_deref(), Some("2026-09-03T10:00:00Z"));
    }

    #[test]
    fn turning_finished_with_no_last_read_time_uses_the_sync_time() {
        let finished = read(100, ReadStatus::Finished, None);
        let row = next_row(None, &finished, NOW).unwrap();
        assert_eq!(row.finished_at.as_deref(), Some(NOW));
    }

    #[test]
    fn a_finished_book_keeps_its_date_until_it_is_finished_again() {
        let finished = read(100, ReadStatus::Finished, Some("2026-09-03T10:00:00Z"));
        let was_finished = row(&finished, Some("2026-09-03T10:00:00Z"));
        // Still finished, more time spent: the date stays.
        let longer = Progress {
            time_spent: Some(700),
            ..finished.clone()
        };
        let row_1 = next_row(Some(&was_finished), &longer, NOW).unwrap();
        assert_eq!(row_1.finished_at.as_deref(), Some("2026-09-03T10:00:00Z"));
        // Opened again: the date carries forward.
        let reopened = read(10, ReadStatus::Reading, Some("2026-09-10T10:00:00Z"));
        let row_2 = next_row(Some(&was_finished), &reopened, NOW).unwrap();
        assert_eq!(row_2.finished_at.as_deref(), Some("2026-09-03T10:00:00Z"));
        // Finished a second time: the date moves.
        let again = read(100, ReadStatus::Finished, Some("2026-09-20T10:00:00Z"));
        let reopened_row = row(&reopened, Some("2026-09-03T10:00:00Z"));
        let row_3 = next_row(Some(&reopened_row), &again, NOW).unwrap();
        assert_eq!(row_3.finished_at.as_deref(), Some("2026-09-20T10:00:00Z"));
    }
}
