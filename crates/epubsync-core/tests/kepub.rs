use epubsync_epub::fixtures as common;

use epubsync_core::kepub;

#[test]
fn converts_an_epub_to_kepub() {
    let dir = tempfile::tempdir().unwrap();
    let input = common::write_epub(dir.path(), "book.epub", common::EPUB2_OPF);
    let output = dir.path().join("book.kepub.epub");

    kepub::convert(&input, &output).unwrap();

    let xhtml = common::read_entry(&output, "OEBPS/chapter1.xhtml");
    assert!(xhtml.contains("koboSpan"), "no koboSpan in:\n{xhtml}");
    assert_eq!(
        common::read_entry(&output, "mimetype"),
        "application/epub+zip"
    );
}

#[test]
fn reports_a_conversion_error() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("not-a-zip.epub");
    std::fs::write(&input, b"hello").unwrap();
    let output = dir.path().join("out.kepub.epub");

    let err = kepub::convert(&input, &output).unwrap_err();
    assert!(err.to_string().starts_with("kepubify: "), "{err}");
}
