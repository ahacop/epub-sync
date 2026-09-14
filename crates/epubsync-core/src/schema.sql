-- The library database. One file in the library folder, applied at init.

CREATE TABLE books (
    id INTEGER PRIMARY KEY,
    revision INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL,
    series TEXT,
    series_number REAL,
    publisher TEXT,
    description TEXT
);

-- The authors of a book in display order.
CREATE TABLE book_authors (
    book_id INTEGER NOT NULL REFERENCES books(id),
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    sort TEXT NOT NULL,
    PRIMARY KEY (book_id, position)
);

-- Numbers about a book's text, read from the file's OPF. The app never
-- edits them or writes them back. A book with no known number has no row.
CREATE TABLE book_stats (
    book_id INTEGER PRIMARY KEY REFERENCES books(id),
    word_count INTEGER,
    reading_ease REAL
);

CREATE TABLE devices (
    serial TEXT PRIMARY KEY
);

-- What the device folder holds: the revision last sent per book per device.
CREATE TABLE sent (
    book_id INTEGER NOT NULL,
    device_serial TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY (book_id, device_serial)
);

-- Reading progress read back from the device, per book per device.
CREATE TABLE progress (
    book_id INTEGER NOT NULL,
    device_serial TEXT NOT NULL,
    percent INTEGER NOT NULL,
    status INTEGER NOT NULL,
    last_read TEXT,
    PRIMARY KEY (book_id, device_serial)
);

-- Words looked up in the device dictionary. Never deleted. A word from a
-- library book carries the book id; every word keeps the device's volume
-- id and the book title so it still reads well after the book is gone.
CREATE TABLE words (
    id INTEGER PRIMARY KEY,
    word TEXT NOT NULL,
    device_serial TEXT NOT NULL,
    book_id INTEGER,
    volume_id TEXT NOT NULL,
    book_title TEXT,
    dict_suffix TEXT,
    looked_up_at TEXT NOT NULL,
    UNIQUE (word, volume_id, looked_up_at)
);

-- The schema version. `Library::open` brings an older database up to it.
PRAGMA user_version = 1;
