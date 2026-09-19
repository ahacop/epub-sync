//! Runs the binary against a temp library.

use std::io::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

const OPF: &str = r##"<?xml version='1.0' encoding='utf-8'?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="uuid_id" opf:scheme="uuid">a1b2c3</dc:identifier>
    <dc:title>The Left Hand of Darkness</dc:title>
    <dc:creator opf:role="aut">Ursula K. Le Guin</dc:creator>
    <dc:language>en</dc:language>
    <meta name="calibre:series" content="Hainish Cycle"/>
    <meta name="calibre:series_index" content="4"/>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

fn write_epub(dir: &Path, name: &str, opf: &str) -> PathBuf {
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;
    let path = dir.join(name);
    let file = std::fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();
    zw.start_file("META-INF/container.xml", deflated).unwrap();
    zw.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    zw.start_file("OEBPS/content.opf", deflated).unwrap();
    zw.write_all(opf.as_bytes()).unwrap();
    zw.start_file("OEBPS/chapter1.xhtml", deflated).unwrap();
    zw.write_all(br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title></head><body><p>Hello.</p></body></html>"#).unwrap();
    zw.finish().unwrap();
    path
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("epubsync").unwrap();
        cmd.env("EPUBSYNC_CONFIG", self.dir.path().join("config.toml"));
        cmd.env_remove("EDITOR");
        cmd
    }

    fn init(&self) {
        self.cmd()
            .args(["init", self.dir.path().join("library").to_str().unwrap()])
            .assert()
            .success();
    }
}

#[test]
fn every_command_but_init_needs_the_config() {
    let env = Env::new();
    env.cmd()
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Run `epubsync init <folder>`"));
}

#[test]
fn init_import_list_edit_remove() {
    let env = Env::new();
    env.init();
    assert!(env.dir.path().join("library/library.sqlite").exists());

    let epub = write_epub(env.dir.path(), "lhod.epub", OPF);
    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("    1  The Left Hand of Darkness"))
        .stdout(predicate::str::contains(
            "made sort name for Ursula K. Le Guin: Le Guin, Ursula K.",
        ));
    assert!(env.dir.path().join("library/1.kepub.epub").exists());

    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("already in the library, skipped"));

    env.cmd()
        .arg("list")
        .assert()
        .success()
        .stdout("    1  The Left Hand of Darkness  by Ursula K. Le Guin  [Hainish Cycle #4]\n");

    env.cmd()
        .args([
            "edit",
            "1",
            "--title",
            "The Left Hand",
            "--series-number",
            "4.5",
            "--author",
            "Ursula K. Le Guin|Le Guin, Ursula",
        ])
        .assert()
        .success()
        .stdout("    1  The Left Hand  by Ursula K. Le Guin  [Hainish Cycle #4.5]\n");

    env.cmd()
        .args(["edit", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("set $EDITOR"));

    env.cmd()
        .args(["remove", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
    assert!(env.dir.path().join("library/1.kepub.epub").exists());

    env.cmd()
        .args(["remove", "1", "--yes"])
        .assert()
        .success()
        .stdout("removed 1 \"The Left Hand\"\n");
    assert!(!env.dir.path().join("library/1.kepub.epub").exists());
    env.cmd().arg("list").assert().success().stdout("");
}

#[test]
fn imports_a_folder() {
    let env = Env::new();
    env.init();
    let books = env.dir.path().join("books");
    std::fs::create_dir(&books).unwrap();
    write_epub(&books, "a.epub", OPF);
    write_epub(
        &books,
        "b.epub",
        &OPF.replace("The Left Hand of Darkness", "The Dispossessed"),
    );
    std::fs::write(books.join("notes.txt"), "not a book").unwrap();
    env.cmd()
        .args(["import", books.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("    1  The Left Hand of Darkness"))
        .stdout(predicate::str::contains("    2  The Dispossessed"));
}

#[test]
fn edits_through_the_editor() {
    let env = Env::new();
    env.init();
    let epub = write_epub(env.dir.path(), "lhod.epub", OPF);
    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success();

    // An "editor" that rewrites the title with sed.
    let editor = env.dir.path().join("editor.sh");
    std::fs::write(
        &editor,
        "#!/bin/sh\nsed -i 's/The Left Hand of Darkness/Winter/' \"$1\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();

    env.cmd()
        .env("EDITOR", editor.to_str().unwrap())
        .args(["edit", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "    1  Winter  by Ursula K. Le Guin",
        ));
}

#[test]
fn syncs_to_a_folder_that_looks_like_a_kobo() {
    let env = Env::new();
    env.init();
    let epub = write_epub(env.dir.path(), "lhod.epub", OPF);
    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success();

    let kobo = env.dir.path().join("KOBOeReader");
    std::fs::create_dir_all(kobo.join(".kobo")).unwrap();
    std::fs::write(
        kobo.join(".kobo/version"),
        "N4181A,3.0.35,4.38.23171,3.0.35,3.0.35,0\n",
    )
    .unwrap();
    std::fs::create_dir_all(kobo.join("EpubSync")).unwrap();
    std::fs::write(kobo.join("EpubSync/42.kepub.epub"), b"stale").unwrap();

    env.cmd()
        .args(["sync", "--device", kobo.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Kobo N4181A at"))
        .stdout(predicate::str::contains(
            "send            1  The Left Hand of Darkness",
        ))
        .stdout(predicate::str::contains(
            "delete         42  (no longer in the library)",
        ))
        .stdout(predicate::str::contains("eject").not());
    assert!(!kobo.join("EpubSync/1.kepub.epub").exists());

    env.cmd()
        .args(["sync", "--device", kobo.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));

    env.cmd()
        .args(["sync", "--device", kobo.to_str().unwrap(), "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sending 1"))
        .stdout(predicate::str::contains("deleting 42"))
        .stdout(predicate::str::contains(
            "run `epubsync eject` before you unplug the device",
        ));
    assert!(kobo.join("EpubSync/1.kepub.epub").exists());
    assert!(!kobo.join("EpubSync/42.kepub.epub").exists());

    env.cmd()
        .args(["sync", "--device", kobo.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to do"));
}

#[test]
fn shows_one_book() {
    let env = Env::new();
    env.init();
    let opf = OPF.replace(
        "<dc:language>en</dc:language>",
        "<dc:language>en</dc:language>
    <dc:publisher>Ace</dc:publisher>
    <dc:description>&lt;p&gt;A &lt;em&gt;human&lt;/em&gt; envoy.&lt;/p&gt;</dc:description>",
    );
    let epub = write_epub(env.dir.path(), "lhod.epub", &opf);
    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success();

    env.cmd()
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Device     not yet sent to a device\n",
        ));

    let db = rusqlite::Connection::open(env.dir.path().join("library/library.sqlite")).unwrap();
    db.execute_batch(
        "INSERT INTO devices (serial) VALUES ('N1');
         INSERT INTO progress (book_id, device_serial, percent, status, last_read) VALUES
           (1, 'N1', 37, 1, '2026-09-01T10:00:00Z');",
    )
    .unwrap();
    drop(db);

    let out = env
        .cmd()
        .args(["show", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    let file = env.dir.path().join("library/1.kepub.epub");
    for line in [
        "Id         1\n",
        "Title      The Left Hand of Darkness\n",
        "Author     Ursula K. Le Guin (sort: Le Guin, Ursula K.)\n",
        "Publisher  Ace\nSeries     Hainish Cycle, book 4\nWords      1\n",
        "Ease       ",
        "Revision   1\n",
        &format!("File       {}\n", file.display()),
        "Device     N1: 37% reading 2026-09-01\n",
        "\nA *human* envoy.\n",
    ] {
        assert!(out.contains(line), "{line:?} not in:\n{out}");
    }

    env.cmd()
        .args(["show", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no book with id 2"));
}

#[test]
fn lists_progress_and_words() {
    let env = Env::new();
    env.init();
    let epub = write_epub(env.dir.path(), "lhod.epub", OPF);
    env.cmd()
        .args(["import", epub.to_str().unwrap()])
        .assert()
        .success();

    let db = rusqlite::Connection::open(env.dir.path().join("library/library.sqlite")).unwrap();
    db.execute_batch(
        "INSERT INTO devices (serial) VALUES ('N1'), ('N2');
         INSERT INTO progress (book_id, device_serial, percent, status, last_read) VALUES
           (1, 'N1', 37, 1, '2026-09-01T10:00:00Z'),
           (1, 'N2', 100, 2, '2026-08-01T10:00:00Z');
         INSERT INTO words (word, device_serial, book_id, volume_id, book_title, dict_suffix, looked_up_at) VALUES
           ('ansible', 'N1', 1, 'file:///mnt/onboard/EpubSync/1.kepub.epub', 'The Left Hand of Darkness', '-en', '2026-09-02T08:00:00Z'),
           ('kemmer', 'N1', 1, 'file:///mnt/onboard/EpubSync/1.kepub.epub', 'The Left Hand of Darkness', '-en', '2026-09-02T09:00:00Z'),
           ('serendipity', 'N2', NULL, 'store-volume', 'A Store Book', '-en', '2026-09-03T10:00:00Z');",
    )
    .unwrap();
    drop(db);

    env.cmd().arg("list").assert().success().stdout(
        "    1  The Left Hand of Darkness  by Ursula K. Le Guin  [Hainish Cycle #4]  N1: 37% reading 2026-09-01  N2: 100% finished 2026-08-01\n",
    );

    let out = env
        .cmd()
        .arg("words")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].starts_with("2026-09-03T10:00:00Z  serendipity"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains("    -  A Store Book  (N2)"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("2026-09-02T09:00:00Z  kemmer"),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].contains("    1  The Left Hand of Darkness  (N1)"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("2026-09-02T08:00:00Z  ansible"),
        "{}",
        lines[2]
    );

    env.cmd()
        .args(["words", "--book", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("serendipity").not())
        .stdout(predicate::str::contains("ansible"));
    env.cmd()
        .args(["words", "--device", "N2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("serendipity"))
        .stdout(predicate::str::contains("ansible").not());
}

#[test]
fn eject_skips_a_folder_that_is_not_a_volume() {
    let env = Env::new();
    env.init();
    let kobo = env.dir.path().join("KOBOeReader");
    std::fs::create_dir_all(kobo.join(".kobo")).unwrap();
    std::fs::write(
        kobo.join(".kobo/version"),
        "N4181A,3.0.35,4.38.23171,3.0.35,3.0.35,0\n",
    )
    .unwrap();
    env.cmd()
        .args(["eject", "--device", kobo.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "not a mounted volume; nothing to eject",
        ));
}
