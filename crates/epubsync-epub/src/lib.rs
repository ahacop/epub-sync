//! Reads and writes the EPUB files EpubSync keeps: the metadata record, the
//! word count and reading ease, and the book text. `Epub` is the whole
//! interface. The crates above it never see the zip, the OPF, or the XML.
//!
//! The files come from many publishers, and the layers under `Epub` absorb
//! what those files do:
//!
//! - a DOCTYPE line in `container.xml` or the OPF
//! - percent-encoded hrefs, such as `Other%2001.xhtml` for `Other 01.xhtml`
//! - a Dublin Core element in default-namespace form instead of `dc:`
//! - the EPUB 2 `opf:file-as` attribute and the EPUB 3 `file-as`
//!   refinement, alone or both on one creator
//! - the Calibre series metas and the EPUB 3 collection form
//! - named entities such as `&nbsp;` in chapters, which the XHTML DTD
//!   defines and an XML parser without the DTD cannot resolve
//! - the blank page kepubify puts first in a converted file
//!
//! A write replaces only the bytes of the elements the app owns and keeps
//! every other byte of the OPF and every other zip entry as it was.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

mod archive;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
pub mod metadata;
mod opf;
mod splice;
mod text;

pub use metadata::{Author, Metadata, Series, Stats};

/// One EPUB or KEPUB file on disk with its OPF read.
#[derive(Debug, Clone)]
pub struct Epub {
    path: PathBuf,
    opf: opf::Opf,
}

impl Epub {
    /// Opens the file and reads its OPF.
    pub fn open(path: &Path) -> Result<Epub> {
        let opf = opf::read(path).with_context(|| format!("read {}", path.display()))?;
        Ok(Epub {
            path: path.to_path_buf(),
            opf,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The metadata record, and the authors whose sort name `make_sort`
    /// made because the file gives none.
    pub fn metadata(&self, make_sort: impl Fn(&str) -> String) -> (Metadata, Vec<Author>) {
        let record = Metadata::from_opf(&self.opf, make_sort);
        let made_sort = self
            .opf
            .creators
            .iter()
            .zip(&record.authors)
            .filter(|(c, _)| c.sort().is_none())
            .map(|(_, a)| a.clone())
            .collect();
        (record, made_sort)
    }

    /// The word count and reading ease the file carries.
    pub fn stats(&self) -> Stats {
        Stats::from_opf(&self.opf)
    }

    /// The `dc:language` value, as written.
    pub fn language(&self) -> Option<&str> {
        self.opf.language.as_deref()
    }

    /// Path of the cover image inside the zip.
    pub fn cover_path(&self) -> Option<&str> {
        self.opf.cover_path.as_deref()
    }

    /// The text of the book: every text node under `body` in every spine
    /// document, in spine order, with a space between nodes and a newline
    /// between documents. The nav document, `script`, and `style` are left
    /// out.
    pub fn text(&self) -> Result<String> {
        text::read(&self.path, &self.opf.spine)
    }

    /// Writes the record and the stats into the file. Owned elements are
    /// replaced in place, missing ones inserted, and the rest of the file
    /// stays as it was. The file is rebuilt in a temp file next to it and
    /// renamed over it.
    pub fn write(&self, metadata: &Metadata, stats: &Stats) -> Result<()> {
        let new_opf = splice::splice(&self.opf, metadata, stats);
        archive::rewrite(&self.path, &self.opf.path, &new_opf)
    }
}
