//! Rebuilds an EPUB zip with one entry replaced. Every other entry is
//! copied raw, so its compressed bytes, its order, and its method stay.

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// Writes `new_opf` in place of the entry at `opf_path` inside the EPUB at
/// `path`. The new zip is built in a temp file next to the original and
/// renamed over it. `mimetype` stays first and stored because it is copied
/// raw in its original position.
pub fn rewrite(path: &Path, opf_path: &str, new_opf: &str) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("read {}", path.display()))?;

    let temp_path = path.with_extension("tmp");
    let temp =
        File::create(&temp_path).with_context(|| format!("create {}", temp_path.display()))?;
    let mut writer = zip::ZipWriter::new(BufWriter::new(temp));

    let mut wrote_opf = false;
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .with_context(|| format!("read entry {i}"))?;
        if entry.name() == opf_path {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(entry.last_modified().unwrap_or_default());
            writer.start_file(opf_path, options)?;
            writer.write_all(new_opf.as_bytes())?;
            wrote_opf = true;
        } else {
            writer.raw_copy_file(entry)?;
        }
    }
    anyhow::ensure!(wrote_opf, "no {opf_path} in {}", path.display());

    let mut out = writer.finish()?;
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);
    std::fs::rename(&temp_path, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}
