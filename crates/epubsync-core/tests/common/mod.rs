//! Builds EPUB fixtures in memory. Each test passes its own OPF text, so one
//! helper covers EPUB 2, EPUB 3, and every metadata form.

#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

pub const OPF_PATH: &str = "OEBPS/content.opf";

pub const CONTAINER_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"##;

pub const CHAPTER_XHTML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<h1>Chapter 1</h1>
<p>It was a dark and stormy night. The rain fell in torrents.</p>
</body>
</html>
"##;

/// A small EPUB 2 OPF in the shape Calibre writes.
pub const EPUB2_OPF: &str = r##"<?xml version='1.0' encoding='utf-8'?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="uuid_id" opf:scheme="uuid">a1b2c3</dc:identifier>
    <dc:title>The Left Hand of Darkness</dc:title>
    <dc:creator opf:file-as="Le Guin, Ursula K." opf:role="aut">Ursula K. Le Guin</dc:creator>
    <dc:language>en</dc:language>
    <dc:publisher>Ace Books</dc:publisher>
    <dc:description>&lt;p&gt;A novel of Winter.&lt;/p&gt;</dc:description>
    <meta name="calibre:series" content="Hainish Cycle"/>
    <meta name="calibre:series_index" content="4"/>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

pub const TOC_NCX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="a1b2c3"/></head>
  <docTitle><text>Book</text></docTitle>
  <navMap>
    <navPoint id="n1" playOrder="1"><navLabel><text>Chapter 1</text></navLabel><content src="chapter1.xhtml"/></navPoint>
  </navMap>
</ncx>
"##;

/// A 1x1 JPEG, enough for a cover entry.
pub const COVER_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
    0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12,
    0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20,
    0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27,
    0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01,
    0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
    0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F,
    0x00, 0x7F, 0xFF, 0xD9,
];

/// Builds an EPUB zip in memory with `mimetype` first and stored, then
/// `container.xml`, the OPF at `OEBPS/content.opf`, one XHTML chapter, an
/// NCX, and a cover image.
pub fn build_epub(opf: &str) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();
    zw.start_file("META-INF/container.xml", deflated).unwrap();
    zw.write_all(CONTAINER_XML.as_bytes()).unwrap();
    zw.start_file(OPF_PATH, deflated).unwrap();
    zw.write_all(opf.as_bytes()).unwrap();
    zw.start_file("OEBPS/chapter1.xhtml", deflated).unwrap();
    zw.write_all(CHAPTER_XHTML.as_bytes()).unwrap();
    zw.start_file("OEBPS/toc.ncx", deflated).unwrap();
    zw.write_all(TOC_NCX.as_bytes()).unwrap();
    zw.start_file("OEBPS/cover.jpg", stored).unwrap();
    zw.write_all(COVER_JPEG).unwrap();
    zw.finish().unwrap().into_inner()
}

/// Writes the EPUB built from `opf` to `name` inside `dir` and returns its path.
pub fn write_epub(dir: &Path, name: &str, opf: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, build_epub(opf)).unwrap();
    path
}

/// Reads one entry out of a zip file as a string.
pub fn read_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
    text
}

/// An EPUB 3 OPF with refinements and a `belongs-to-collection` series.
pub const EPUB3_OPF: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="en">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:uuid:d4e5f6</dc:identifier>
    <dc:title id="t1">A Wizard of Earthsea</dc:title>
    <dc:creator id="creator01">Ursula K. Le Guin</dc:creator>
    <meta refines="#creator01" property="role" scheme="marc:relators">aut</meta>
    <meta refines="#creator01" property="file-as">Le Guin, Ursula K.</meta>
    <dc:language>en</dc:language>
    <dc:publisher>Parnassus Press</dc:publisher>
    <dc:description>Ged the sparrowhawk.</dc:description>
    <meta property="dcterms:modified">2024-01-01T00:00:00Z</meta>
    <meta property="belongs-to-collection" id="c01">Earthsea Cycle</meta>
    <meta refines="#c01" property="collection-type">series</meta>
    <meta refines="#c01" property="group-position">1</meta>
  </metadata>
  <manifest>
    <item id="cover" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

/// An EPUB 3 OPF as Calibre writes it: both `file-as` forms on one creator
/// and the Calibre series metas.
pub const EPUB3_CALIBRE_OPF: &str = r##"<?xml version='1.0' encoding='utf-8'?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="3.0" prefix="calibre: https://calibre-ebook.com">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="uuid_id">urn:uuid:112233</dc:identifier>
    <dc:title>The Dispossessed</dc:title>
    <dc:creator id="id" opf:file-as="Le Guin, Ursula K." opf:role="aut">Ursula K. Le Guin</dc:creator>
    <meta refines="#id" property="file-as">Le Guin, Ursula K.</meta>
    <meta refines="#id" property="role" scheme="marc:relators">aut</meta>
    <dc:language>en</dc:language>
    <dc:publisher>Harper &amp; Row</dc:publisher>
    <dc:description>An ambiguous utopia.</dc:description>
    <meta name="calibre:series" content="Hainish Cycle"/>
    <meta name="calibre:series_index" content="5"/>
    <meta property="dcterms:modified">2024-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="cover" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

/// An EPUB 2 OPF whose `description` carries the Dublin Core namespace as
/// its own default namespace instead of the `dc:` prefix.
pub const DEFAULT_NS_DESCRIPTION_OPF: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="bookid">isbn-0</dc:identifier>
    <dc:title>The Lathe of Heaven</dc:title>
    <dc:creator opf:file-as="Le Guin, Ursula K.">Ursula K. Le Guin</dc:creator>
    <dc:language>en</dc:language>
    <description xmlns="http://purl.org/dc/elements/1.1/">Dreams that change the world.</description>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

/// An EPUB 3 OPF with two titles, the second marked `main`.
pub const TWO_TITLES_OPF: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:uuid:aa</dc:identifier>
    <dc:title id="t1">Earthsea</dc:title>
    <meta refines="#t1" property="title-type">collection</meta>
    <dc:title id="t2">The Tombs of Atuan</dc:title>
    <meta refines="#t2" property="title-type">main</meta>
    <dc:creator id="a1">Ursula K. Le Guin</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

/// An EPUB 2 OPF with no series, no publisher, no description, no `file-as`,
/// and no `opf` prefix declared.
pub const BARE_OPF: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">isbn-1</dc:identifier>
    <dc:title>Candide</dc:title>
    <dc:creator>Voltaire</dc:creator>
    <dc:language>fr</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>
"##;

pub const ALL_OPFS: &[(&str, &str)] = &[
    ("epub2", EPUB2_OPF),
    ("epub3", EPUB3_OPF),
    ("epub3-calibre", EPUB3_CALIBRE_OPF),
    ("default-ns-description", DEFAULT_NS_DESCRIPTION_OPF),
    ("two-titles", TWO_TITLES_OPF),
    ("bare", BARE_OPF),
];
