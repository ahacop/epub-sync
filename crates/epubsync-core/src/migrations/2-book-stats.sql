-- Migration 2: numbers about a book's text, read from the file's OPF. The
-- app never edits them or writes them back. A book with no known number
-- has no row. The migration hook fills the table from the book files.
CREATE TABLE book_stats (
    book_id INTEGER PRIMARY KEY REFERENCES books(id),
    word_count INTEGER,
    reading_ease REAL
);
