//! The library folder: the book files, the SQLite database, and the lock.
//! Every command opens the library once and holds the lock until it ends.

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::config::{self, Config};
use crate::metadata::{Author, Metadata, Series};
use crate::sort_name::sort_name;
use crate::{epub, kepub, opf, splice};

const SCHEMA: &str = include_str!("schema.sql");
const DB_NAME: &str = "library.sqlite";
const LOCK_NAME: &str = "lock";
const IMPORT_TEMP: &str = "import.tmp";

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
    /// Creates the folder, the database from the schema, and the config
    /// file that points at the folder.
    pub fn init(folder: &Path) -> Result<Library> {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
        let folder = folder.canonicalize()?;
        let db_path = folder.join(DB_NAME);
        if db_path.exists() {
            bail!("{} already holds a library", folder.display());
        }
        let db =
            Connection::open(&db_path).with_context(|| format!("create {}", db_path.display()))?;
        db.execute_batch(SCHEMA).context("apply the schema")?;
        drop(db);
        let config = Config {
            library: folder.clone(),
        };
        config::save(&config)?;
        Library::open(&config)
    }

    /// Opens the library named by the config, taking the exclusive lock.
    pub fn open(config: &Config) -> Result<Library> {
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
        let db =
            Connection::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Library {
            folder,
            db,
            _lock: guard,
        })
    }

    /// The path of a book's file in the library folder.
    pub fn book_path(&self, id: i64) -> PathBuf {
        self.folder.join(format!("{id}.kepub.epub"))
    }

    /// Imports an EPUB or KEPUB. See the design for the six steps.
    pub fn import(&mut self, source: &Path, force: bool) -> Result<ImportOutcome> {
        // 1 and 2: read the file metadata and make missing sort names.
        let source_opf = opf::read(source).with_context(|| format!("read {}", source.display()))?;
        let record = Metadata::from_opf(&source_opf, sort_name);
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
        let id = self.insert(&record)?;
        std::fs::rename(&temp, self.book_path(id)).context("rename the imported file")?;

        // 6: write made sort names into the converted file.
        if !made_sort.is_empty() {
            self.write_file(id, &record)?;
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

    fn insert(&mut self, record: &Metadata) -> Result<i64> {
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
        tx.commit()?;
        Ok(id)
    }

    /// Splices the record into the book's file.
    fn write_file(&self, id: i64, record: &Metadata) -> Result<()> {
        let path = self.book_path(id);
        let file_opf = opf::read(&path)?;
        let new_opf = splice::splice(&file_opf, record);
        epub::rewrite(&path, &file_opf.path, &new_opf)
    }

    pub fn list(&self) -> Result<Vec<Book>> {
        let mut stmt = self.db.prepare(
            "SELECT id, revision, title, series, series_number, publisher, description FROM books ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Book {
                id: row.get(0)?,
                revision: row.get(1)?,
                metadata: Metadata {
                    title: row.get(2)?,
                    authors: Vec::new(),
                    series: row
                        .get::<_, Option<String>>(3)?
                        .map(|name| Series { name, number: None }),
                    publisher: row.get(5)?,
                    description: row.get(6)?,
                },
            })
            .map(|mut b: Book| {
                if let Some(s) = &mut b.metadata.series {
                    s.number = row.get(4).ok().flatten();
                }
                b
            })
        })?;
        let mut books = Vec::new();
        for book in rows {
            let mut book = book?;
            book.metadata.authors = self.authors(book.id)?;
            books.push(book);
        }
        Ok(books)
    }

    pub fn get(&self, id: i64) -> Result<Book> {
        self.list()?
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| anyhow!("no book with id {id}"))
    }

    fn authors(&self, id: i64) -> Result<Vec<Author>> {
        let mut stmt = self
            .db
            .prepare("SELECT name, sort FROM book_authors WHERE book_id = ?1 ORDER BY position")?;
        let rows = stmt.query_map([id], |row| {
            Ok(Author {
                name: row.get(0)?,
                sort: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Updates the row, adds 1 to the revision, then writes the record into
    /// the file, in that order.
    pub fn edit(&mut self, id: i64, record: &Metadata) -> Result<()> {
        self.get(id)?;
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
        self.write_file(id, record)
    }

    /// Deletes the file, then the book row, its authors, its `sent` rows,
    /// and its `progress` rows. Word rows stay.
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
        tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}

fn insert_authors(tx: &rusqlite::Transaction, id: i64, authors: &[Author]) -> Result<()> {
    for (position, author) in authors.iter().enumerate() {
        tx.execute(
            "INSERT INTO book_authors (book_id, position, name, sort) VALUES (?1, ?2, ?3, ?4)",
            params![id, position as i64, author.name, author.sort],
        )?;
    }
    Ok(())
}

/// Reading progress for one book on one device.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressRow {
    pub book_id: i64,
    pub device_serial: String,
    pub percent: i64,
    pub status: i64,
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
    /// Every progress row, in book id then device order.
    pub fn progress(&self) -> Result<Vec<ProgressRow>> {
        let mut stmt = self.db.prepare(
            "SELECT book_id, device_serial, percent, status, last_read FROM progress ORDER BY book_id, device_serial",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ProgressRow {
                book_id: r.get(0)?,
                device_serial: r.get(1)?,
                percent: r.get(2)?,
                status: r.get(3)?,
                last_read: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
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
