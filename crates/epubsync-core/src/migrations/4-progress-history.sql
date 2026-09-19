-- Migration 4: progress is a history, and the `progress` view is its
-- newest row per book per device.
--
-- Sync adds a row each time what it reads for a book on a device differs
-- from the newest row. Rows are never deleted or changed. `seen_at` is
-- the time of the sync that read the row. `time_spent` is the Kobo's
-- reading time in seconds. `finished_at` is the `last_read` of the row
-- whose status first read as finished, copied to every row after it, and
-- replaced when the status turns finished again.

CREATE TABLE progress_history (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id),
    device_serial TEXT NOT NULL,
    percent INTEGER NOT NULL,
    status INTEGER NOT NULL,
    last_read TEXT,
    time_spent INTEGER,
    finished_at TEXT,
    seen_at TEXT NOT NULL
);

CREATE INDEX progress_history_newest ON progress_history (book_id, device_serial, id);

-- A finished row's finished date is its last read time. A progress row
-- whose book id has no `books` row is dropped.
INSERT INTO progress_history (book_id, device_serial, percent, status, last_read, finished_at, seen_at)
    SELECT book_id, device_serial, percent, status, last_read,
           CASE WHEN status = 2 THEN last_read END,
           strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
    FROM progress
    WHERE book_id IN (SELECT id FROM books)
    ORDER BY book_id, device_serial;

DROP TABLE progress;

-- The newest history row per book per device: the current progress.
CREATE VIEW progress AS
    SELECT book_id, device_serial, percent, status, last_read, time_spent, finished_at, seen_at
    FROM progress_history h
    WHERE id = (SELECT max(id) FROM progress_history
                WHERE book_id = h.book_id AND device_serial = h.device_serial);
