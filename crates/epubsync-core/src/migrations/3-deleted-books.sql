-- Migration 3: removed books keep their row, and words get the book from it.
--
-- `remove` sets `deleted_at` instead of deleting the row. The
-- `active_books` view holds the books that are not removed, and every
-- list of books reads from it. A word row points at a `books` row, so
-- the word keeps its book title after the book is removed.

ALTER TABLE books ADD COLUMN deleted_at TEXT;

CREATE VIEW active_books AS
    SELECT id, revision, title, series, series_number, publisher, description
    FROM books WHERE deleted_at IS NULL;

-- Words looked up in the device dictionary. Never deleted. The title and
-- the volume id come from the book row, so a word from a book that is
-- not in the library has no row.
CREATE TABLE words_new (
    id INTEGER PRIMARY KEY,
    word TEXT NOT NULL,
    device_serial TEXT NOT NULL,
    book_id INTEGER NOT NULL REFERENCES books(id),
    dict_suffix TEXT,
    looked_up_at TEXT NOT NULL,
    UNIQUE (word, book_id, looked_up_at)
);

-- A word row whose book id has no `books` row is dropped.
INSERT OR IGNORE INTO words_new (id, word, device_serial, book_id, dict_suffix, looked_up_at)
    SELECT id, word, device_serial, book_id, dict_suffix, looked_up_at FROM words
    WHERE book_id IN (SELECT id FROM books)
    ORDER BY id;

DROP TABLE words;
ALTER TABLE words_new RENAME TO words;
