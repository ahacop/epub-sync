//! A Kobo over USB mass storage: a mounted volume with a `.kobo/version`
//! file. Books go into the `EpubSync` folder at the root of the volume as
//! `<id>.kepub.epub`. The Kobo database is opened in place for the row
//! updates and the read back.

pub mod db;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::device::{Action, Device, ReadBack, RowUpdate};
use crate::metadata::Metadata;
use db::KoboDb;

pub const FOLDER: &str = "EpubSync";
const VERSION_FILE: &str = ".kobo/version";

pub struct Kobo {
    pub serial: String,
    pub root: PathBuf,
    db: Option<KoboDb>,
    allow_untested: bool,
}

/// The mount folders detect scans: `/run/media/$USER`, `/media`,
/// `/media/$USER`, and `/Volumes`.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/media"), PathBuf::from("/Volumes")];
    if let Ok(user) = std::env::var("USER") {
        roots.insert(0, PathBuf::from("/run/media").join(&user));
        roots.insert(2, PathBuf::from("/media").join(&user));
    }
    roots
}

/// Finds every mounted volume directly under one of `roots` that has a
/// `.kobo/version` file.
pub fn detect(roots: &[PathBuf]) -> Vec<Kobo> {
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            if let Ok(kobo) = Kobo::at(&path) {
                found.push(kobo);
            }
        }
    }
    found
}

impl Kobo {
    /// Opens the volume at `root`, which must hold `.kobo/version`. The
    /// first field of that file is the serial.
    pub fn at(root: &Path) -> Result<Kobo> {
        let version_path = root.join(VERSION_FILE);
        let text = std::fs::read_to_string(&version_path)
            .with_context(|| format!("{} is not a Kobo: no {VERSION_FILE}", root.display()))?;
        let serial = text.split(',').next().unwrap_or("").trim().to_string();
        if serial.is_empty() {
            return Err(anyhow!("{} has an empty serial", version_path.display()));
        }
        Ok(Kobo {
            serial,
            root: root.to_path_buf(),
            db: None,
            allow_untested: false,
        })
    }

    /// Opens `.kobo/KoboReader.sqlite` when the volume has one. With
    /// `allow_untested` the row writes run on any database version.
    pub fn open_db(&mut self, allow_untested: bool) -> Result<()> {
        let path = self.root.join(db::DB_PATH);
        if path.exists() {
            self.db = Some(KoboDb::open(&path)?);
        }
        self.allow_untested = allow_untested;
        Ok(())
    }

    pub fn db_version(&self) -> Option<i64> {
        self.db.as_ref().map(|d| d.version)
    }

    fn writes_allowed(&self) -> bool {
        match &self.db {
            Some(db) => db.is_tested() || self.allow_untested,
            None => false,
        }
    }

    pub fn folder(&self) -> PathBuf {
        self.root.join(FOLDER)
    }

    pub fn book_path(&self, id: i64) -> PathBuf {
        self.folder().join(format!("{id}.kepub.epub"))
    }

    /// The path the firmware keys the book's rows by.
    pub fn volume_id(&self, id: i64) -> String {
        format!("file:///mnt/onboard/{FOLDER}/{id}.kepub.epub")
    }
}

/// Parses the book id out of a device file name such as `12.kepub.epub`.
pub fn id_from_file_name(name: &str) -> Option<i64> {
    name.strip_suffix(".kepub.epub")?.parse().ok()
}

/// Parses the book id out of a volume id such as
/// `file:///mnt/onboard/EpubSync/12.kepub.epub`.
pub fn id_from_volume_id(volume_id: &str) -> Option<i64> {
    let name = volume_id.strip_prefix(&format!("file:///mnt/onboard/{FOLDER}/"))?;
    id_from_file_name(name)
}

impl Device for Kobo {
    fn serial(&self) -> &str {
        &self.serial
    }

    fn list(&self) -> Result<Vec<i64>> {
        let entries = match std::fs::read_dir(self.folder()) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.folder().display())),
        };
        let mut ids: Vec<i64> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| id_from_file_name(&e.file_name().to_string_lossy()))
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn apply(&mut self, action: &Action, source: &Path) -> Result<()> {
        let id = action.id();
        let target = self.book_path(id);
        let volume_id = self.volume_id(id);
        let writes = self.writes_allowed();
        match action {
            Action::Send { .. } | Action::Replace { .. } | Action::SendAgain { .. } => {
                std::fs::create_dir_all(self.folder())?;
                let size = std::fs::copy(source, &target)
                    .with_context(|| format!("copy to {}", target.display()))?;
                std::fs::File::open(&target)?.sync_all()?;
                if matches!(action, Action::Replace { .. })
                    && writes
                    && let Some(db) = &self.db
                {
                    db.update_file_size(&volume_id, size as i64)?;
                }
            }
            Action::Delete { .. } => {
                std::fs::remove_file(&target)
                    .with_context(|| format!("delete {}", target.display()))?;
            }
        }
        Ok(())
    }

    fn write_gate(&self) -> Option<String> {
        match &self.db {
            Some(db) if !db.is_tested() && !self.allow_untested => Some(format!(
                "Kobo database version {} has not been tested. Replacements and row updates are skipped. \
                 Pass --allow-newer-firmware to run them.",
                db.version
            )),
            _ => None,
        }
    }

    fn update_rows(&mut self, records: &[(i64, &Metadata)]) -> Result<Vec<(i64, RowUpdate)>> {
        let Some(db) = &mut self.db else {
            return Ok(records
                .iter()
                .map(|(id, _)| (*id, RowUpdate::NoRow))
                .collect());
        };
        let tx = db.begin()?;
        let mut results = Vec::new();
        for (id, record) in records {
            let volume_id = format!("file:///mnt/onboard/{FOLDER}/{id}.kepub.epub");
            results.push((*id, db::update_metadata(&tx, &volume_id, record)?));
        }
        tx.commit()?;
        Ok(results)
    }

    fn read_back(&mut self, book_ids: &[i64]) -> Result<ReadBack> {
        let Some(db) = &self.db else {
            return Ok(ReadBack::default());
        };
        let mut progress = Vec::new();
        for id in book_ids {
            if let Some(p) = db.progress(&self.volume_id(*id), *id)? {
                progress.push(p);
            }
        }
        let words =
            db.words(|volume_id| id_from_volume_id(volume_id).filter(|id| book_ids.contains(id)))?;
        Ok(ReadBack { progress, words })
    }

    fn finish(&mut self) -> Result<()> {
        if let Some(db) = self.db.take() {
            db.close()?;
        }
        Ok(())
    }
}
