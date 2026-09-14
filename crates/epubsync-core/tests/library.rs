use std::path::Path;

use epubsync_core::config::Config;
use epubsync_core::library::{ImportOutcome, Library, Stats};
use epubsync_core::metadata::{Author, Series};
use epubsync_epub::Epub;
use epubsync_epub::fixtures as common;

struct Setup {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    config: Config,
}

/// Makes a temp folder with the config file inside it and inits a library
/// in a `library` subfolder. The config path is set through the env var
/// for the length of the test process.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let config_path = root.join("config.toml");
    // Each test process runs one test at a time here: see the serial lock.
    unsafe { std::env::set_var("EPUBSYNC_CONFIG", &config_path) };
    let lib = Library::init(&root.join("library")).unwrap();
    let config = Config {
        library: lib.folder.clone(),
    };
    drop(lib);
    Setup {
        _dir: dir,
        root,
        config,
    }
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The first author's sort name as the file gives it, `None` when the
/// file gives none.
fn file_sort(path: &Path) -> Option<String> {
    let (record, made) = Epub::open(path).unwrap().metadata(str::to_string);
    made.is_empty().then(|| record.authors[0].sort.clone())
}

#[test]
fn init_then_open_and_a_second_open_fails_on_the_lock() {
    let _s = serial();
    let s = setup();
    assert!(s.config.library.join("library.sqlite").exists());
    assert_eq!(
        toml::from_str::<Config>(&std::fs::read_to_string(s.root.join("config.toml")).unwrap())
            .unwrap()
            .library,
        s.config.library
    );

    let first = Library::open(&s.config).unwrap();
    let err = match Library::open(&s.config) {
        Ok(_) => panic!("second open took the lock"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("another EpubSync is running"),
        "{err}"
    );
    drop(first);
    Library::open(&s.config).unwrap();
}

#[test]
fn imports_an_epub_and_writes_the_made_sort_name() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "candide.epub", common::BARE_OPF);

    let outcome = lib.import(&source, false).unwrap();
    let ImportOutcome::Imported { id, made_sort } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(id, 1);
    assert_eq!(
        made_sort,
        vec![Author {
            name: "Voltaire".into(),
            sort: "Voltaire".into()
        }]
    );

    let path = lib.book_path(id);
    assert_eq!(path.file_name().unwrap(), "1.kepub.epub");
    assert!(path.exists());
    assert!(!lib.folder.join("import.tmp").exists());
    assert!(common::read_entry(&path, "OEBPS/chapter1.xhtml").contains("koboSpan"));
    assert_eq!(file_sort(&path).as_deref(), Some("Voltaire"));

    let books = lib.list().unwrap();
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].revision, 1);
    assert_eq!(books[0].metadata.title, "Candide");
    assert_eq!(books[0].metadata.authors[0].sort, "Voltaire");
    assert!(books[0].metadata.series.is_none());

    // The source file is unchanged.
    assert_eq!(
        std::fs::read(&source).unwrap(),
        common::build_epub(common::BARE_OPF)
    );
}

#[test]
fn import_reads_every_field_and_leaves_a_file_with_sort_names_alone() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "lhod.epub", common::EPUB2_OPF);
    let ImportOutcome::Imported { id, made_sort } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    assert!(made_sort.is_empty());
    let book = lib.get(id).unwrap();
    assert_eq!(book.metadata.title, "The Left Hand of Darkness");
    assert_eq!(
        book.metadata.authors,
        vec![Author {
            name: "Ursula K. Le Guin".into(),
            sort: "Le Guin, Ursula K.".into()
        }]
    );
    assert_eq!(
        book.metadata.series,
        Some(Series {
            name: "Hainish Cycle".into(),
            number: Some(4.0)
        })
    );
    assert_eq!(book.metadata.publisher.as_deref(), Some("Ace Books"));
    assert_eq!(
        book.metadata.description.as_deref(),
        Some("<p>A novel of Winter.</p>")
    );
}

/// A chapter with eleven words in two sentences.
const SHORT_CHAPTER: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<p>The cat sat on the mat. It was a <i>sunny</i> day.</p>
</body>
</html>
"##;

const SHORT_STATS: Stats = Stats {
    word_count: Some(11),
    reading_ease: Some(109.0),
};

#[test]
fn import_keeps_the_numbers_a_standard_ebooks_file_carries() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "pp.epub", common::STANDARD_EBOOKS_OPF);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    assert_eq!(
        lib.get(id).unwrap().stats,
        Stats {
            word_count: Some(121970),
            reading_ease: Some(60.95),
        }
    );
    // The file keeps the numbers Standard Ebooks wrote.
    let text = common::read_entry(&lib.book_path(id), common::OPF_PATH);
    assert!(text.contains(r#"<meta property="schema:wordCount">121970</meta>"#));
    assert!(text.contains(r#"<meta property="schema:educationalLevel">60.95</meta>"#));
}

#[test]
fn import_measures_a_file_with_no_word_count() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_book(&s.root, "lhod.epub", common::EPUB2_OPF, SHORT_CHAPTER);
    let ImportOutcome::Imported { id, made_sort } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    assert!(made_sort.is_empty());
    assert_eq!(lib.get(id).unwrap().stats, SHORT_STATS);

    // A book in a language the crate has no Flesch coefficients for gets
    // a word count and no reading ease.
    let latin = common::EPUB2_OPF
        .replace(
            "<dc:language>en</dc:language>",
            "<dc:language>la</dc:language>",
        )
        .replace("The Left Hand of Darkness", "De Bello Gallico");
    let source = common::write_book(&s.root, "bello.epub", &latin, SHORT_CHAPTER);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    assert_eq!(
        lib.get(id).unwrap().stats,
        Stats {
            word_count: Some(11),
            reading_ease: None,
        }
    );
}

#[test]
fn import_writes_the_measured_numbers_into_the_library_file() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_book(&s.root, "lhod.epub", common::EPUB2_OPF, SHORT_CHAPTER);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    let path = lib.book_path(id);
    // The conversion re-indents the OPF, so the lines are matched trimmed.
    let text = common::read_entry(&path, common::OPF_PATH);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines
        .iter()
        .position(|l| *l == r#"<meta property="schema:wordCount">11</meta>"#)
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(
        lines[at + 1],
        r#"<meta property="schema:educationalLevel">109.00</meta>"#,
        "{text}"
    );
    assert_eq!(Epub::open(&path).unwrap().stats(), SHORT_STATS);
    assert!(common::read_entry(&path, "OEBPS/chapter1.xhtml").contains("koboSpan"));

    // An edit writes the same numbers again.
    let mut record = lib.get(id).unwrap().metadata;
    record.title = "The Left Hand".into();
    lib.edit(id, &record).unwrap();
    assert_eq!(
        common::read_entry(&path, common::OPF_PATH),
        text.replace("The Left Hand of Darkness", "The Left Hand")
    );
}

#[test]
fn stops_on_the_same_title_and_author_unless_forced() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "lhod.epub", common::EPUB2_OPF);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    assert_eq!(
        lib.import(&source, false).unwrap(),
        ImportOutcome::Exists { id }
    );
    assert_eq!(lib.list().unwrap().len(), 1);

    let ImportOutcome::Imported { id: second, .. } = lib.import(&source, true).unwrap() else {
        panic!();
    };
    assert_ne!(second, id);
    assert_eq!(lib.list().unwrap().len(), 2);
    assert!(lib.book_path(second).exists());
}

#[test]
fn copies_a_kepub_without_converting() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "lhod.kepub.epub", common::EPUB2_OPF);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    // The chapter is copied as it is. The OPF gets the measured numbers.
    let unconverted = |path: &Path| {
        assert_eq!(
            common::read_entry(path, "OEBPS/chapter1.xhtml"),
            common::CHAPTER_XHTML
        );
        assert!(
            common::read_entry(path, common::OPF_PATH)
                .contains(r#"<meta property="schema:wordCount">13</meta>"#)
        );
    };
    unconverted(&lib.book_path(id));

    // A file named .kepub, as some publishers ship Kobo builds, is a KEPUB too.
    let other = common::EPUB2_OPF.replace("The Left Hand of Darkness", "The Dispossessed");
    let source = common::write_epub(&s.root, "other.kepub", &other);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    unconverted(&lib.book_path(id));
}

#[test]
fn a_failed_conversion_leaves_no_row_and_no_temp_file() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    // The chapter is in the manifest and not in the spine, so measuring
    // skips it and the conversion is the first step that reads it.
    let opf = common::EPUB2_OPF.replace(r#"<itemref idref="ch1"/>"#, "");
    let source = common::write_epub(&s.root, "broken.epub", &opf);
    common::corrupt_entry(&source, "OEBPS/chapter1.xhtml");
    let err = lib.import(&source, false).unwrap_err();
    assert!(err.to_string().contains("convert"), "{err}");
    assert!(lib.list().unwrap().is_empty());
    assert!(!lib.folder.join("import.tmp").exists());
}

#[test]
fn edit_updates_the_row_the_revision_and_the_file() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "lhod.epub", common::EPUB2_OPF);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    let mut record = lib.get(id).unwrap().metadata;
    record.title = "The Left Hand".into();
    record.authors[0].sort = "Le Guin, U. K.".into();
    record.series = Some(Series {
        name: "Hainish".into(),
        number: Some(4.5),
    });
    record.publisher = None;
    lib.edit(id, &record).unwrap();

    let book = lib.get(id).unwrap();
    assert_eq!(book.revision, 2);
    assert_eq!(book.metadata, record);

    let (file, made) = Epub::open(&lib.book_path(id))
        .unwrap()
        .metadata(str::to_string);
    assert!(made.is_empty());
    assert_eq!(file, record);
    assert!(common::read_entry(&lib.book_path(id), "OEBPS/chapter1.xhtml").contains("koboSpan"));
    assert_eq!(
        common::read_entry(&lib.book_path(id), "mimetype"),
        "application/epub+zip"
    );
}

#[test]
fn remove_deletes_the_file_and_the_rows_but_keeps_words() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "lhod.epub", common::EPUB2_OPF);
    let ImportOutcome::Imported { id, .. } = lib.import(&source, false).unwrap() else {
        panic!();
    };
    lib.db
        .execute("INSERT INTO devices (serial) VALUES ('N123')", [])
        .unwrap();
    lib.db
        .execute(
            "INSERT INTO sent (book_id, device_serial, revision) VALUES (?1, 'N123', 1)",
            [id],
        )
        .unwrap();
    lib.db
        .execute("INSERT INTO progress (book_id, device_serial, percent, status, last_read) VALUES (?1, 'N123', 50, 1, '2026-01-01')", [id])
        .unwrap();
    lib.db
        .execute(
            "INSERT INTO words (word, device_serial, book_id, volume_id, book_title, dict_suffix, looked_up_at)
             VALUES ('ansible', 'N123', ?1, 'file:///mnt/onboard/EpubSync/1.kepub.epub', 'The Left Hand of Darkness', '-en', '2026-01-01')",
            [id],
        )
        .unwrap();

    let path = lib.book_path(id);
    lib.remove(id).unwrap();
    assert!(!path.exists());
    assert!(lib.list().unwrap().is_empty());
    let count = |table: &str| -> i64 {
        lib.db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("book_authors"), 0);
    assert_eq!(count("book_stats"), 0);
    assert_eq!(count("sent"), 0);
    assert_eq!(count("progress"), 0);
    assert_eq!(count("words"), 1);
    assert!(lib.remove(id).is_err());
}
