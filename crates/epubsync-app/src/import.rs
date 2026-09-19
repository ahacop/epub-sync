//! Import from the viewer: the Import button and a drop of files onto the
//! window. Each file goes through `Library::import` on a background task,
//! because kepubify takes seconds per book and the measurement takes
//! more. The task takes the `Library` value with it and hands it back
//! with the outcome, so the state holds no library while a file imports.
//! The book's row lands in the table after each file, and a strip under
//! the toolbar shows the count so far and every file that was skipped or
//! failed.

use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use epubsync_core::library::{ImportOutcome, Library};
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Center, Element, Fill, Task, padding};

use crate::theme::{BODY, MONO, SANS_MEDIUM};
use crate::{Message, theme};

/// A library value on its way back from the import task. A message must
/// be Clone and Debug, and a Library is neither, so the task hands the
/// library back inside this handle and the update takes it out.
#[derive(Clone)]
pub struct Handoff(Arc<Mutex<Option<Library>>>);

impl Handoff {
    fn new(library: Library) -> Handoff {
        Handoff(Arc::new(Mutex::new(Some(library))))
    }

    /// Takes the library out. A second take gets None.
    pub fn take(&self) -> Option<Library> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl fmt::Debug for Handoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Handoff")
    }
}

/// What one file's import came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// The book is in the library.
    Added { title: String },
    /// A book with the same title and first author is already in the
    /// library, as book `id`.
    Skipped { id: i64, file: PathBuf },
    /// The import failed. `error` is the whole error chain.
    Failed { file: PathBuf, error: String },
}

/// An import under way or done: the files still to go, the one in
/// flight, and a line per file done.
#[derive(Debug, Clone, Default)]
pub struct Import {
    queue: VecDeque<PathBuf>,
    current: Option<PathBuf>,
    lines: Vec<Line>,
}

impl Import {
    /// Whether a file is on the task now.
    pub fn running(&self) -> bool {
        self.current.is_some()
    }

    /// Queues a path. A folder gives its `.epub` files one level deep,
    /// in name order, the same as the CLI, and a folder with none gets a
    /// failed line. Any other path is queued as one file.
    pub fn add(&mut self, path: &Path) {
        if !path.is_dir() {
            self.queue.push_back(path.to_path_buf());
            return;
        }
        match epubs_in(path) {
            Ok(files) if files.is_empty() => self.lines.push(Line::Failed {
                file: path.to_path_buf(),
                error: "no .epub files in the folder".to_string(),
            }),
            Ok(files) => self.queue.extend(files),
            Err(e) => self.lines.push(Line::Failed {
                file: path.to_path_buf(),
                error: format!("{e:#}"),
            }),
        }
    }

    /// Starts the next queued file when none is in flight and the state
    /// holds the library. The task takes the library and gives it back
    /// with the file's line in `Message::Imported`.
    pub fn start(&mut self, library: &mut Option<Library>) -> Task<Message> {
        if self.current.is_some() || self.queue.is_empty() || library.is_none() {
            return Task::none();
        }
        let file = self.queue.pop_front().expect("the queue is not empty");
        let mut lib = library.take().expect("the state holds the library");
        self.current = Some(file.clone());
        Task::perform(
            async move {
                let line = import_one(&mut lib, file);
                (Handoff::new(lib), line)
            },
            |(handoff, line)| Message::Imported(handoff, line),
        )
    }

    /// Records the line of the file that was in flight.
    pub fn finish(&mut self, line: Line) {
        self.current = None;
        self.lines.push(line);
    }
}

fn import_one(lib: &mut Library, file: PathBuf) -> Line {
    match lib.import(&file, false) {
        Ok(ImportOutcome::Imported { id, .. }) => {
            // The title comes from the row the import made. If the read
            // fails, the file name stands in.
            let title = lib
                .get(id)
                .map(|b| b.metadata.title)
                .unwrap_or_else(|_| file_name(&file));
            Line::Added { title }
        }
        Ok(ImportOutcome::Exists { id }) => Line::Skipped { id, file },
        Err(e) => Line::Failed {
            file,
            error: format!("{e:#}"),
        },
    }
}

/// The `.epub` files in a folder, one level deep, in name order.
fn epubs_in(folder: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".epub"))
        .collect();
    files.sort();
    Ok(files)
}

/// Opens the system file picker on EPUB files. The picked paths come
/// back in `Message::Picked`; a cancel gives none.
pub fn pick() -> Task<Message> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_title("Import EPUB files")
        .add_filter("EPUB", &["epub", "kepub"]);
    Task::perform(dialog.pick_files(), |picked| {
        let paths = picked
            .unwrap_or_default()
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect();
        Message::Picked(paths)
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// "Imported 10 books · 1 skipped · 2 failed". A count of zero is left
/// out, so an import with nothing skipped or failed reads "Imported 10
/// books".
fn summary(lines: &[Line]) -> String {
    let added = lines
        .iter()
        .filter(|l| matches!(l, Line::Added { .. }))
        .count();
    let skipped = lines
        .iter()
        .filter(|l| matches!(l, Line::Skipped { .. }))
        .count();
    let failed = lines
        .iter()
        .filter(|l| matches!(l, Line::Failed { .. }))
        .count();
    let mut parts = vec![format!("Imported {}", crate::count(added, "book"))];
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    parts.join(" · ")
}

/// The strip under the toolbar. While a file is in flight it reads
/// "Importing 3 of 12 · pride.epub". When the queue is done it reads the
/// summary, with a line for each skipped or failed file under it and a ×
/// that clears the strip. Added books need no line: their rows are in
/// the table.
pub fn view(import: &Import) -> Element<'_, Message> {
    let head: Element<'_, Message> = match &import.current {
        Some(file) => {
            let n = import.lines.len() + 1;
            let total = n + import.queue.len();
            row![
                text(format!("Importing {n} of {total}"))
                    .size(BODY)
                    .font(SANS_MEDIUM),
                text(file_name(file))
                    .size(BODY)
                    .style(theme::text_color(|c| c.muted)),
            ]
            .spacing(10)
            .into()
        }
        None => {
            let close = button(container(text("×").size(15)).center(22))
                .on_press(Message::ClearImport)
                .padding(0)
                .style(theme::close);
            row![
                text(summary(&import.lines)).size(BODY).font(SANS_MEDIUM),
                space().width(Fill),
                close,
            ]
            .align_y(Center)
            .into()
        }
    };
    let notes = import.lines.iter().filter_map(|line| match line {
        Line::Added { .. } => None,
        Line::Skipped { id, file } => Some(note(
            "Skipped",
            file,
            format!("already in the library as book {id}"),
        )),
        Line::Failed { file, error } => Some(note("Failed", file, error.clone())),
    });
    let body = column![head]
        .push(scrollable(column(notes).spacing(2)))
        .spacing(6)
        .padding(padding::all(10).left(14).right(8));
    column![
        container(body)
            .width(Fill)
            .max_height(180)
            .style(theme::ground(|c| c.window)),
        theme::hline(),
    ]
    .into()
}

/// One note line: the word, the file name, and what happened.
fn note<'a>(word: &'a str, file: &Path, what: String) -> Element<'a, Message> {
    row![
        text(word)
            .size(12)
            .width(52)
            .style(theme::text_color(|c| c.muted)),
        text(file_name(file)).size(12).font(MONO),
        text(what).size(12).style(theme::text_color(|c| c.ink_2)),
    ]
    .spacing(8)
    .into()
}

/// The strip while files hover over the window with no import in view.
pub fn drop_hint<'a>() -> Element<'a, Message> {
    column![
        container(
            text("Drop EPUB files to import them")
                .size(BODY)
                .font(SANS_MEDIUM)
                .style(theme::text_color(|c| c.accent))
        )
        .width(Fill)
        .padding(padding::all(10).left(14))
        .style(theme::ground(|c| c.accent_tint)),
        theme::hline(),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: u8) -> Line {
        match kind {
            0 => Line::Added {
                title: "A".to_string(),
            },
            1 => Line::Skipped {
                id: 4,
                file: PathBuf::from("a.epub"),
            },
            _ => Line::Failed {
                file: PathBuf::from("b.epub"),
                error: "bad".to_string(),
            },
        }
    }

    #[test]
    fn summary_counts_each_kind_and_drops_zeros() {
        assert_eq!(summary(&[]), "Imported 0 books");
        assert_eq!(summary(&[line(0)]), "Imported 1 book");
        assert_eq!(
            summary(&[line(0), line(0), line(1), line(2), line(2)]),
            "Imported 2 books · 1 skipped · 2 failed"
        );
    }

    #[test]
    fn add_queues_a_file_and_expands_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.epub", "a.epub", "notes.txt"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/c.epub"), b"").unwrap();

        let mut import = Import::default();
        import.add(&dir.path().join("notes.txt"));
        import.add(dir.path());
        let queued: Vec<PathBuf> = import.queue.iter().cloned().collect();
        assert_eq!(
            queued,
            [
                dir.path().join("notes.txt"),
                dir.path().join("a.epub"),
                dir.path().join("b.epub"),
            ]
        );
        assert!(import.lines.is_empty());
    }

    #[test]
    fn add_of_an_empty_folder_is_a_failed_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut import = Import::default();
        import.add(dir.path());
        assert!(import.queue.is_empty());
        assert_eq!(
            import.lines,
            [Line::Failed {
                file: dir.path().to_path_buf(),
                error: "no .epub files in the folder".to_string(),
            }]
        );
    }
}

#[cfg(test)]
mod library_tests {
    use super::*;
    use epubsync_epub::fixtures;

    /// The three lines one file can come to, against a real library:
    /// added on the first import, skipped on the second because the
    /// title and first author match, and failed for a file that is
    /// not an EPUB.
    #[test]
    fn import_one_gives_each_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::init(&dir.path().join("library")).unwrap();
        let epub = fixtures::write_epub(dir.path(), "lhod.epub", fixtures::EPUB2_OPF);
        let junk = dir.path().join("junk.epub");
        std::fs::write(&junk, b"not a zip").unwrap();

        assert_eq!(
            import_one(&mut lib, epub.clone()),
            Line::Added {
                title: "The Left Hand of Darkness".to_string()
            }
        );
        assert_eq!(
            import_one(&mut lib, epub.clone()),
            Line::Skipped {
                id: 1,
                file: epub.clone()
            }
        );
        let Line::Failed { file, error } = import_one(&mut lib, junk.clone()) else {
            panic!("junk imported");
        };
        assert_eq!(file, junk);
        assert!(!error.is_empty());
        assert_eq!(lib.list().unwrap().len(), 1);
    }
}
