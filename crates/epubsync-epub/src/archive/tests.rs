use super::rewrite;
use crate::fixtures as common;
use crate::opf;
use zip::CompressionMethod;

fn entries(path: &std::path::Path) -> Vec<(String, CompressionMethod, u32)> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    (0..archive.len())
        .map(|i| {
            let e = archive.by_index_raw(i).unwrap();
            (e.name().to_string(), e.compression(), e.crc32())
        })
        .collect()
}

#[test]
fn rewrite_keeps_every_other_entry_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_epub(dir.path(), "book.epub", common::EPUB2_OPF);
    let before = entries(&path);
    assert_eq!(before.len(), 6);
    assert_eq!(before[0].0, "mimetype");
    assert_eq!(before[0].1, CompressionMethod::Stored);
    assert_eq!(before[5].1, CompressionMethod::Stored);

    let new_opf = common::EPUB2_OPF.replace("The Left Hand of Darkness", "The Right Hand of Light");
    rewrite(&path, common::OPF_PATH, &new_opf).unwrap();

    let after = entries(&path);
    assert_eq!(after.len(), before.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(a.0, b.0, "entry order");
        assert_eq!(a.1, b.1, "compression of {}", b.0);
        if b.0 != common::OPF_PATH {
            assert_eq!(a.2, b.2, "crc of {}", b.0);
        }
    }
    assert_eq!(after[0].0, "mimetype");
    assert_eq!(after[0].1, CompressionMethod::Stored);
    assert_eq!(common::read_entry(&path, common::OPF_PATH), new_opf);
    assert_eq!(
        common::read_entry(&path, "mimetype"),
        "application/epub+zip"
    );
    assert_eq!(
        opf::read(&path).unwrap().title.unwrap().value,
        "The Right Hand of Light"
    );
    assert!(!dir.path().join("book.tmp").exists());
}
