-- Migration 2: book ids are never used again.
--
-- A book id names the library file, the device file, and the word rows
-- that outlive the book. With AUTOINCREMENT, SQLite gives a new book an
-- id above every id it has given out, so a new book never takes the id
-- of a removed one. SQLite cannot add AUTOINCREMENT to a table, so this
-- copies the rows into a new table.

CREATE TABLE books_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    revision INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL,
    series TEXT,
    series_number REAL,
    publisher TEXT,
    description TEXT
);

INSERT INTO books_new (id, revision, title, series, series_number, publisher, description)
    SELECT id, revision, title, series, series_number, publisher, description FROM books;

DROP TABLE books;
ALTER TABLE books_new RENAME TO books;

-- The ids of books removed before this migration are gone, except where a
-- word row still holds one. The next id starts above those too.
DELETE FROM sqlite_sequence WHERE name = 'books';
INSERT INTO sqlite_sequence (name, seq)
    SELECT 'books', max(
        (SELECT coalesce(max(id), 0) FROM books),
        (SELECT coalesce(max(book_id), 0) FROM words)
    );
