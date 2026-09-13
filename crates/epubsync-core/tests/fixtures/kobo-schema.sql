-- PLACEHOLDER. Replace this file with the output of
--     sqlite3 /path/to/KOBOeReader/.kobo/KoboReader.sqlite .schema
-- from a real Kobo, and record its dbversion in kobo::db::TESTED_VERSIONS.
-- Until then the tests run against these tables, which hold only the
-- columns the app reads and writes. The column names come from Calibre's
-- Kobo driver.

CREATE TABLE dbversion(version INTEGER);

CREATE TABLE content(
    ContentID TEXT NOT NULL,
    ContentType TEXT NOT NULL,
    MimeType TEXT NOT NULL DEFAULT 'application/x-kobo-epub+zip',
    BookID TEXT,
    BookTitle TEXT,
    Title TEXT COLLATE NOCASE,
    Attribution TEXT COLLATE NOCASE,
    Description TEXT,
    Publisher TEXT,
    DateLastRead TEXT,
    ReadStatus INTEGER DEFAULT 0,
    ___FileSize INTEGER DEFAULT 0,
    ___PercentRead INTEGER DEFAULT 0,
    Accessibility INTEGER DEFAULT 1,
    IsDownloaded BIT NOT NULL DEFAULT 1,
    Series TEXT,
    SeriesNumber TEXT,
    SeriesID TEXT,
    SeriesNumberFloat REAL,
    PRIMARY KEY (ContentID)
);

CREATE TABLE WordList(
    Text TEXT NOT NULL,
    VolumeId TEXT NOT NULL,
    DictSuffix TEXT,
    DateCreated TEXT,
    PRIMARY KEY (Text, VolumeId)
);
