mod common;

use std::path::Path;

use epubsync_core::config::Config;
use epubsync_core::library::{ImportOutcome, Library};
use epubsync_core::metadata::{Author, Series};
use epubsync_core::opf;

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

fn file_sort(path: &Path) -> Option<String> {
    opf::read(path).unwrap().creators[0]
        .sort()
        .map(str::to_string)
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
    assert_eq!(
        std::fs::read(lib.book_path(id)).unwrap(),
        std::fs::read(&source).unwrap()
    );
}

#[test]
fn a_failed_conversion_leaves_no_row_and_no_temp_file() {
    let _s = serial();
    let s = setup();
    let mut lib = Library::open(&s.config).unwrap();
    let source = common::write_epub(&s.root, "broken.epub", common::EPUB2_OPF);
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

    let file = opf::read(&lib.book_path(id)).unwrap();
    assert_eq!(file.title.unwrap().value, "The Left Hand");
    assert_eq!(file.creators[0].sort(), Some("Le Guin, U. K."));
    assert_eq!(file.series.as_ref().unwrap().number, Some(4.5));
    assert!(file.publisher.is_none());
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
    assert_eq!(count("sent"), 0);
    assert_eq!(count("progress"), 0);
    assert_eq!(count("words"), 1);
    assert!(lib.remove(id).is_err());
}
