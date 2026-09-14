//! Runs a sync against one device: plan, apply with the `sent` table
//! updated after each book, then read progress and words back.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::params;

use crate::device::{Action, Device, ReadBack, RowUpdate};
use crate::library::Library;

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

/// Reads progress and words from the device and stores them. Progress
/// rows go in one transaction, word rows in another. A word the library
/// already holds is skipped.
pub fn read_back(library: &mut Library, device: &mut dyn Device) -> Result<ReadBack> {
    let serial = device.serial().to_string();
    let ids: Vec<i64> = library.list()?.iter().map(|b| b.id).collect();
    let back = device.read_back(&ids)?;

    let tx = library.db.transaction()?;
    for p in &back.progress {
        tx.execute(
            "INSERT INTO progress (book_id, device_serial, percent, status, last_read) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (book_id, device_serial) DO UPDATE
             SET percent = excluded.percent, status = excluded.status, last_read = excluded.last_read",
            params![p.book_id, serial, p.percent, p.status, p.last_read],
        )?;
    }
    tx.commit()?;

    let tx = library.db.transaction()?;
    let mut new_words = Vec::new();
    for w in &back.words {
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO words (word, device_serial, book_id, volume_id, book_title, dict_suffix, looked_up_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![w.word, serial, w.book_id, w.volume_id, w.book_title, w.dict_suffix, w.looked_up_at],
        )?;
        if inserted > 0 {
            new_words.push(w.clone());
        }
    }
    tx.commit()?;
    Ok(ReadBack {
        progress: back.progress,
        words: new_words,
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
