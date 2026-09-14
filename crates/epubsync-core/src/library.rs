//! The library folder: the book files, the SQLite database, and the lock.
//! Every command opens the library once and holds the lock until it ends.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use rusqlite_migration::{HookError, HookResult, M, Migrations};

use crate::config::{self, Config};
use crate::device::ReadStatus;
use crate::metadata::{Author, Metadata, Series};
use crate::sort_name::sort_name;
use crate::stats::Engines;
pub use crate::stats::Stats;
use crate::{epub, kepub, opf, splice, stats};

const TABLES_SQL: &str = include_str!("migrations/1-tables.sql");
const BOOK_STATS_SQL: &str = include_str!("migrations/2-book-stats.sql");
const MEASURE_BOOKS_SQL: &str = include_str!("migrations/3-measure-books.sql");
const DB_NAME: &str = "library.sqlite";
const LOCK_NAME: &str = "lock";
const IMPORT_TEMP: &str = "import.tmp";

/// Receives one line per step of a migration that takes a while, such as
/// the one that measures every book. The CLI prints the lines.
pub type Report = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Library {
    pub folder: PathBuf,
    pub db: Connection,
    /// The exclusive lock on the lock file. It is released when the
    /// Library drops, at the end of the command.
    _lock: fd_lock::RwLockWriteGuard<'static, File>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: i64,
    pub revision: i64,
    pub metadata: Metadata,
    pub stats: Stats,
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
    /// Creates the folder and the database, and writes the config file
    /// that points at the folder.
    pub fn init(folder: &Path) -> Result<Library> {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
        let folder = folder.canonicalize()?;
        if folder.join(DB_NAME).exists() {
            bail!("{} already holds a library", folder.display());
        }
        let config = Config { library: folder };
        let lib = Library::open(&config)?;
        config::save(&config)?;
        Ok(lib)
    }

    /// Opens the library named by the config, taking the exclusive lock,
    /// and runs the migrations the database is missing. A missing
    /// database file is created and gets every migration. Migration
    /// progress is not reported.
    pub fn open(config: &Config) -> Result<Library> {
        Library::open_reporting(config, |_| {})
    }

    /// Opens the library like `open`, and passes each progress line of a
    /// long migration to `report`.
    pub fn open_reporting(
        config: &Config,
        report: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<Library> {
        let folder = config.library.clone();
        let lock_path = folder.join(LOCK_NAME);
        let lock_file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open {}", lock_path.display()))?;
        // The guard borrows the lock for as long as it lives, so the lock
        // is leaked to give the guard a static lifetime. One command opens
        // one library, so the leak is one small struct per process.
        let lock: &'static mut fd_lock::RwLock<File> =
            Box::leak(Box::new(fd_lock::RwLock::new(lock_file)));
        let guard = match lock.try_write() {
            Ok(guard) => guard,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                bail!("another EpubSync is running on {}", folder.display())
            }
            Err(e) => return Err(e).with_context(|| format!("lock {}", lock_path.display())),
        };
        let db_path = folder.join(DB_NAME);
        let mut db =
            Connection::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;
        stamp_unversioned(&db)?;
        migrations(folder.clone(), Arc::new(report))
            .to_latest(&mut db)
            .context("migrate the database")?;
        // Foreign keys go on after the migrations, as SQLite advises for
        // schema changes.
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Library {
            folder,
            db,
            _lock: guard,
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
        let source_opf = opf::read(source).with_context(|| format!("read {}", source.display()))?;
        let record = Metadata::from_opf(&source_opf, sort_name);
        let file_stats = Stats::from_opf(&source_opf);
        let measured = file_stats.word_count.is_none();
        let stats = if measured {
            stats::measure(source, &source_opf, &mut Engines::default())
                .with_context(|| format!("measure {}", source.display()))?
        } else {
            file_stats
        };
        let made_sort: Vec<Author> = source_opf
            .creators
            .iter()
            .zip(&record.authors)
            .filter(|(c, _)| c.sort().is_none())
            .map(|(_, a)| a.clone())
            .collect();

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
        let path = self.book_path(id);
        let file_opf = opf::read(&path)?;
        let new_opf = splice::splice(&file_opf, record, stats);
        epub::rewrite(&path, &file_opf.path, &new_opf)
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
fn migrations(folder: PathBuf, report: Report) -> Migrations<'static> {
    Migrations::new(vec![
        M::up(TABLES_SQL),
        M::up_with_hook(BOOK_STATS_SQL, {
            let folder = folder.clone();
            move |tx| fill_book_stats(tx, &folder)
        }),
        M::up_with_hook(MEASURE_BOOKS_SQL, move |tx| {
            measure_books(tx, &folder, &report)
        }),
    ])
}

/// Stamps a database from 0.1.6 or earlier as version 1. Those releases
/// made the tables without setting `user_version`, so the database reads
/// as empty to the migrations, and the first migration would fail on the
/// tables that are already there.
fn stamp_unversioned(db: &Connection) -> rusqlite::Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let has_tables: bool = db.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'books')",
        [],
        |r| r.get(0),
    )?;
    if version == 0 && has_tables {
        db.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}

/// Fills `book_stats` from the OPF of every book file. A file that cannot
/// be read gets no row, the same as a file with no numbers in it.
fn fill_book_stats(tx: &Transaction, folder: &Path) -> HookResult {
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM books")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        let Ok(file_opf) = opf::read(&folder.join(book_file_name(id))) else {
            continue;
        };
        insert_stats(tx, id, &Stats::from_opf(&file_opf))?;
    }
    Ok(())
}

/// Measures every book with no `book_stats` row, inserts the row, and
/// writes the numbers into the file. A book whose OPF cannot be read gets
/// no row, as in migration 2, and a line says so. The hook runs inside the
/// migration transaction, so a measurement or a file write that fails
/// rolls the rows back and the next open runs the hook again. A file that
/// got its numbers before the failure keeps them, and the next run writes
/// the same numbers.
fn measure_books(tx: &Transaction, folder: &Path, report: &Report) -> HookResult {
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM books WHERE id NOT IN (SELECT book_id FROM book_stats)")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if ids.is_empty() {
        return Ok(());
    }
    let total = ids.len();
    report(&format!(
        "measuring the word count and reading ease of {total} books"
    ));
    let mut engines = Engines::default();
    for (n, id) in ids.into_iter().enumerate() {
        let path = folder.join(book_file_name(id));
        let file_opf = match opf::read(&path) {
            Ok(file_opf) => file_opf,
            Err(e) => {
                report(&format!("skipped book {id}: {e:#}"));
                continue;
            }
        };
        let record = read_book(tx, id)
            .map_err(|e| HookError::Hook(format!("{e:#}")))?
            .metadata;
        report(&format!("{}/{total} {}", n + 1, record.title));
        let stats = stats::measure(&path, &file_opf, &mut engines)
            .map_err(|e| HookError::Hook(format!("measure book {id}: {e:#}")))?;
        insert_stats(tx, id, &stats)?;
        let new_opf = splice::splice(&file_opf, &record, &stats);
        epub::rewrite(&path, &file_opf.path, &new_opf)
            .map_err(|e| HookError::Hook(format!("write book {id}: {e:#}")))?;
    }
    Ok(())
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
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressRow {
    pub device_serial: String,
    pub percent: i64,
    pub status: ReadStatus,
    pub last_read: Option<String>,
}

/// One looked-up word as the library stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct WordRow {
    pub word: String,
    pub device_serial: String,
    pub book_id: Option<i64>,
    pub volume_id: String,
    pub book_title: Option<String>,
    pub dict_suffix: Option<String>,
    pub looked_up_at: String,
}

impl Library {
    /// Every progress row, grouped by book id and in device order within
    /// a book.
    pub fn progress(&self) -> Result<BTreeMap<i64, Vec<ProgressRow>>> {
        let mut stmt = self.db.prepare(
            "SELECT book_id, device_serial, percent, status, last_read FROM progress ORDER BY book_id, device_serial",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                ProgressRow {
                    device_serial: r.get(1)?,
                    percent: r.get(2)?,
                    status: r.get(3)?,
                    last_read: r.get(4)?,
                },
            ))
        })?;
        let mut by_book: BTreeMap<i64, Vec<ProgressRow>> = BTreeMap::new();
        for row in rows {
            let (book_id, progress) = row?;
            by_book.entry(book_id).or_default().push(progress);
        }
        Ok(by_book)
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
        migrations(PathBuf::new(), Arc::new(|_| {}))
            .validate()
            .unwrap();
    }
}
