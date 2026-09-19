//! The library folder: the book files, the SQLite database, and the lock.
//! Every command opens the library once and holds the lock until it ends.

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use rusqlite_migration::{M, Migrations};
use serde::Serialize;

use crate::config::Config;
use crate::device::ReadStatus;
use crate::metadata::{Author, Metadata, Series};
use crate::sort_name::sort_name;
use crate::stats::Engines;
pub use crate::stats::Stats;
use crate::{kepub, stats};
use epubsync_epub::Epub;

const TABLES_SQL: &str = include_str!("migrations/1-tables.sql");
const BOOKS_AUTOINCREMENT_SQL: &str = include_str!("migrations/2-books-autoincrement.sql");
const DB_NAME: &str = "library.sqlite";
const LOCK_NAME: &str = "lock";
const IMPORT_TEMP: &str = "import.tmp";

pub struct Library {
    pub folder: PathBuf,
    pub db: Connection,
    /// The exclusive lock on the lock file. It is released when the
    /// Library drops, at the end of the command.
    _lock: File,
}

/// A book as the library holds it. As JSON it is one flat object: the
/// metadata and the stats fields sit next to `id` and `revision`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Book {
    pub id: i64,
    pub revision: i64,
    #[serde(flatten)]
    pub metadata: Metadata,
    #[serde(flatten)]
    pub stats: Stats,
}

/// One optional detail of a book, with its value as the library stores
/// it. A display picks the label and the text for each one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Field<'a> {
    Publisher(&'a str),
    Series(&'a Series),
    WordCount(u64),
    ReadingEase(f64),
}

impl Book {
    /// The optional details the book has, in the order a display lists
    /// them. A detail the book does not have gets no entry.
    pub fn fields(&self) -> Vec<Field<'_>> {
        let m = &self.metadata;
        [
            m.publisher.as_deref().map(Field::Publisher),
            m.series.as_ref().map(Field::Series),
            self.stats.word_count.map(Field::WordCount),
            self.stats.reading_ease.map(Field::ReadingEase),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportOutcome {
    /// The file is in the library. `made_sort` lists the authors that got
    /// a sort name made from the display name.
    Imported { id: i64, made_sort: Vec<Author> },
    /// A book with the same title and first author is already in the
    /// library, and `force` was not given.
    Exists { id: i64 },
}

impl Library {
    /// Creates the folder and the database.
    pub fn init(folder: &Path) -> Result<Library> {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
        let folder = folder.canonicalize()?;
        if folder.join(DB_NAME).exists() {
            bail!("{} already holds a library", folder.display());
        }
        let config = Config { library: folder };
        let lib = Library::open(&config)?;
        Ok(lib)
    }

    /// Opens the library named by the config, taking the exclusive lock,
    /// and runs the migrations the database is missing. A missing
    /// database file is created and gets every migration.
    pub fn open(config: &Config) -> Result<Library> {
        let folder = config.library.clone();
        let lock_path = folder.join(LOCK_NAME);
        let lock_file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open {}", lock_path.display()))?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                bail!("another EpubSync is running on {}", folder.display())
            }
            Err(TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("lock {}", lock_path.display()));
            }
        }
        let db_path = folder.join(DB_NAME);
        let mut db =
            Connection::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;
        // Foreign keys are off while the migrations run, as SQLite advises
        // for schema changes, and on after. The bundled SQLite turns them
        // on by default, so they are turned off here first. A migration
        // that rebuilds a table runs `foreign_key_check` at its end.
        db.execute_batch("PRAGMA foreign_keys = OFF;")?;
        migrations()
            .to_latest(&mut db)
            .context("migrate the database")?;
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Library {
            folder,
            db,
            _lock: lock_file,
        })
    }

    /// The path of a book's file in the library folder.
    pub fn book_path(&self, id: i64) -> PathBuf {
        self.folder.join(book_file_name(id))
    }

    /// Imports an EPUB or KEPUB. See the design for the six steps.
    pub fn import(&mut self, source: &Path, force: bool) -> Result<ImportOutcome> {
        // 1 and 2: read the file metadata, measure a file that carries no
        // word count, and make missing sort names.
        let source_epub = Epub::open(source)?;
        let (record, made_sort) = source_epub.metadata(sort_name);
        let file_stats = source_epub.stats();
        let measured = file_stats.word_count.is_none();
        let stats = if measured {
            stats::measure(&source_epub, &mut Engines::default())
                .with_context(|| format!("measure {}", source.display()))?
        } else {
            file_stats
        };

        // 3: stop on a book with the same title and first author.
        if !force && let Some(id) = self.find_same(&record)? {
            return Ok(ImportOutcome::Exists { id });
        }

        // 4: convert or copy into a temp file in the library folder.
        let temp = self.folder.join(IMPORT_TEMP);
        let is_kepub = source
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".kepub.epub") || n.ends_with(".kepub"));
        let result = if is_kepub {
            std::fs::copy(source, &temp)
                .map(|_| ())
                .with_context(|| format!("copy {}", source.display()))
        } else {
            kepub::convert(source, &temp).with_context(|| format!("convert {}", source.display()))
        };
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }

        // 5: insert the row and rename the file to its id.
        let id = self.insert(&record, &stats)?;
        std::fs::rename(&temp, self.book_path(id)).context("rename the imported file")?;

        // 6: write made sort names and measured numbers into the converted
        // file.
        if !made_sort.is_empty() || measured {
            self.write_file(id, &record, &stats)?;
        }
        Ok(ImportOutcome::Imported { id, made_sort })
    }

    fn find_same(&self, record: &Metadata) -> Result<Option<i64>> {
        let Some(first) = record.authors.first() else {
            return Ok(None);
        };
        self.db
            .query_row(
                "SELECT b.id FROM books b JOIN book_authors a ON a.book_id = b.id AND a.position = 0
                 WHERE b.title = ?1 AND a.name = ?2 ORDER BY b.id LIMIT 1",
                params![record.title, first.name],
                |row| row.get(0),
            )
            .optional()
            .context("look for the same book")
    }

    fn insert(&mut self, record: &Metadata, stats: &Stats) -> Result<i64> {
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO books (title, series, series_number, publisher, description) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.title,
                record.series.as_ref().map(|s| &s.name),
                record.series.as_ref().and_then(|s| s.number),
                record.publisher,
                record.description,
            ],
        )?;
        let id = tx.last_insert_rowid();
        insert_authors(&tx, id, &record.authors)?;
        insert_stats(&tx, id, stats)?;
        tx.commit()?;
        Ok(id)
    }

    /// Splices the record and the stats into the book's file.
    fn write_file(&self, id: i64, record: &Metadata, stats: &Stats) -> Result<()> {
        Epub::open(&self.book_path(id))?.write(record, stats)
    }

    /// Every book in id order.
    pub fn list(&self) -> Result<Vec<Book>> {
        let mut stmt = self.db.prepare(&format!("{BOOK_SELECT} ORDER BY id"))?;
        let rows = stmt.query_map([], book_from_row)?;
        let mut books = Vec::new();
        for book in rows {
            let mut book = book?;
            book.metadata.authors = self.authors(book.id)?;
            books.push(book);
        }
        Ok(books)
    }

    pub fn get(&self, id: i64) -> Result<Book> {
        read_book(&self.db, id)
    }

    fn authors(&self, id: i64) -> Result<Vec<Author>> {
        read_authors(&self.db, id)
    }

    /// Updates the row, adds 1 to the revision, then writes the record into
    /// the file, in that order. The stats are written as stored, which is
    /// what the file holds.
    pub fn edit(&mut self, id: i64, record: &Metadata) -> Result<()> {
        let stats = self.get(id)?.stats;
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE books SET revision = revision + 1, title = ?2, series = ?3, series_number = ?4,
             publisher = ?5, description = ?6 WHERE id = ?1",
            params![
                id,
                record.title,
                record.series.as_ref().map(|s| &s.name),
                record.series.as_ref().and_then(|s| s.number),
                record.publisher,
                record.description,
            ],
        )?;
        tx.execute("DELETE FROM book_authors WHERE book_id = ?1", [id])?;
        insert_authors(&tx, id, &record.authors)?;
        tx.commit()?;
        self.write_file(id, record, &stats)
    }

    /// Deletes the file, then the book row, its authors, its stats, its
    /// `sent` rows, and its `progress` rows. Word rows stay.
    pub fn remove(&mut self, id: i64) -> Result<()> {
        self.get(id)?;
        let path = self.book_path(id);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("delete {}", path.display())),
        }
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM progress WHERE book_id = ?1", [id])?;
        tx.execute("DELETE FROM sent WHERE book_id = ?1", [id])?;
        tx.execute("DELETE FROM book_authors WHERE book_id = ?1", [id])?;
        tx.execute("DELETE FROM book_stats WHERE book_id = ?1", [id])?;
        tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}

/// The schema changes in order. `PRAGMA user_version` counts how many of
/// them the database has, and `Library::open` runs the rest. Each one
/// runs in a transaction, so a change that fails leaves the database as
/// it was.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(TABLES_SQL),
        M::up(BOOKS_AUTOINCREMENT_SQL).foreign_key_check(),
    ])
}

/// The name of a book's file in the library folder.
pub fn book_file_name(id: i64) -> String {
    format!("{id}.kepub.epub")
}

const BOOK_SELECT: &str =
    "SELECT id, revision, title, series, series_number, publisher, description,
     word_count, reading_ease FROM books LEFT JOIN book_stats ON book_id = id";

fn read_book(db: &Connection, id: i64) -> Result<Book> {
    let mut book = db
        .query_row(&format!("{BOOK_SELECT} WHERE id = ?1"), [id], book_from_row)
        .optional()?
        .ok_or_else(|| anyhow!("no book with id {id}"))?;
    book.metadata.authors = read_authors(db, id)?;
    Ok(book)
}

fn read_authors(db: &Connection, id: i64) -> Result<Vec<Author>> {
    let mut stmt =
        db.prepare("SELECT name, sort FROM book_authors WHERE book_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map([id], |row| {
        Ok(Author {
            name: row.get(0)?,
            sort: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A `books` row joined to its `book_stats` row as a Book with no authors.
/// The caller fills them in from `book_authors`.
fn book_from_row(row: &rusqlite::Row) -> rusqlite::Result<Book> {
    let series_name: Option<String> = row.get(3)?;
    let series_number: Option<f64> = row.get(4)?;
    Ok(Book {
        id: row.get(0)?,
        revision: row.get(1)?,
        metadata: Metadata {
            title: row.get(2)?,
            authors: Vec::new(),
            series: series_name.map(|name| Series {
                name,
                number: series_number,
            }),
            publisher: row.get(5)?,
            description: row.get(6)?,
        },
        stats: Stats {
            word_count: row.get::<_, Option<i64>>(7)?.map(|n| n as u64),
            reading_ease: row.get(8)?,
        },
    })
}

/// Inserts the stats row when there is a number to hold.
fn insert_stats(tx: &Transaction, id: i64, stats: &Stats) -> rusqlite::Result<()> {
    if stats.is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO book_stats (book_id, word_count, reading_ease) VALUES (?1, ?2, ?3)",
        params![id, stats.word_count.map(|n| n as i64), stats.reading_ease],
    )?;
    Ok(())
}

fn insert_authors(tx: &Transaction, id: i64, authors: &[Author]) -> Result<()> {
    for (position, author) in authors.iter().enumerate() {
        tx.execute(
            "INSERT INTO book_authors (book_id, position, name, sort) VALUES (?1, ?2, ?3, ?4)",
            params![id, position as i64, author.name, author.sort],
        )?;
    }
    Ok(())
}

/// Reading progress on one device for the book it is keyed by.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgressRow {
    pub device_serial: String,
    pub percent: i64,
    pub status: ReadStatus,
    pub last_read: Option<String>,
}

impl ProgressRow {
    /// The day part of `last_read`: the first ten characters of the
    /// timestamp, as "2026-09-08".
    pub fn day(&self) -> Option<&str> {
        self.last_read.as_deref().map(|d| d.get(..10).unwrap_or(d))
    }
}

/// Reads the four progress columns that start at column `first`.
fn progress_from_row(r: &rusqlite::Row, first: usize) -> rusqlite::Result<ProgressRow> {
    Ok(ProgressRow {
        device_serial: r.get(first)?,
        percent: r.get(first + 1)?,
        status: r.get(first + 2)?,
        last_read: r.get(first + 3)?,
    })
}

/// One looked-up word as the library stores it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WordRow {
    pub word: String,
    pub device_serial: String,
    pub book_id: Option<i64>,
    pub volume_id: String,
    pub book_title: Option<String>,
    pub dict_suffix: Option<String>,
    pub looked_up_at: String,
}

impl WordRow {
    /// The day part of `looked_up_at`: the first ten characters of the
    /// timestamp, as "2026-09-08".
    pub fn day(&self) -> &str {
        self.looked_up_at.get(..10).unwrap_or(&self.looked_up_at)
    }
}

impl Library {
    /// Every progress row, grouped by book id and in device order within
    /// a book.
    pub fn progress(&self) -> Result<BTreeMap<i64, Vec<ProgressRow>>> {
        let mut stmt = self.db.prepare(
            "SELECT book_id, device_serial, percent, status, last_read FROM progress ORDER BY book_id, device_serial",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, progress_from_row(r, 1)?)))?;
        let mut by_book: BTreeMap<i64, Vec<ProgressRow>> = BTreeMap::new();
        for row in rows {
            let (book_id, progress) = row?;
            by_book.entry(book_id).or_default().push(progress);
        }
        Ok(by_book)
    }

    /// One book's progress rows in device order. A book with no rows, or
    /// no book with that id, gives an empty list.
    pub fn book_progress(&self, book_id: i64) -> Result<Vec<ProgressRow>> {
        let mut stmt = self.db.prepare(
            "SELECT device_serial, percent, status, last_read FROM progress WHERE book_id = ?1 ORDER BY device_serial",
        )?;
        let rows = stmt.query_map([book_id], |r| progress_from_row(r, 0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The looked-up words, newest first, filtered by book id and device
    /// serial when given.
    pub fn words(&self, book_id: Option<i64>, device_serial: Option<&str>) -> Result<Vec<WordRow>> {
        let mut stmt = self.db.prepare(
            "SELECT word, device_serial, book_id, volume_id, book_title, dict_suffix, looked_up_at FROM words
             WHERE (?1 IS NULL OR book_id = ?1) AND (?2 IS NULL OR device_serial = ?2)
             ORDER BY looked_up_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![book_id, device_serial], |r| {
            Ok(WordRow {
                word: r.get(0)?,
                device_serial: r.get(1)?,
                book_id: r.get(2)?,
                volume_id: r.get(3)?,
                book_title: r.get(4)?,
                dict_suffix: r.get(5)?,
                looked_up_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_migrations_apply_to_an_empty_database() {
        migrations().validate().unwrap();
    }
}
