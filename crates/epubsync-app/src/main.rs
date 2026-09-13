//! The library viewer: a window with a table of books and, when a book is
//! selected, its details in a pane on the right. It is read-only. The CLI
//! imports, edits, removes, and syncs.

mod description;
mod format;
mod read;
mod table;
mod theme;

use std::path::PathBuf;

use epubsync_core::config;
use epubsync_core::library::{Library, ProgressRow};
use iced::widget::{column, container, markdown, row, scrollable, space, text, text_input};
use iced::{Center, Element, Fill, Theme, padding};

use crate::read::Entry;
use crate::table::{Column, Sort};
use crate::theme::{BODY, MONO, SANS_SEMIBOLD};

/// The state of the viewer window: what it draws.
#[derive(Debug, Clone)]
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the table.
    Open(Open),
}

#[derive(Debug, Clone)]
struct Open {
    /// The library folder. The status bar shows it.
    folder: PathBuf,
    /// The books in id order. The table sorts a borrowed view.
    books: Vec<Entry>,
    /// Reading progress, one row per book per device.
    progress: Vec<ProgressRow>,
    sort: Sort,
    /// The filter field's text.
    filter: String,
    /// The book in the right pane, if any.
    selected: Option<Selected>,
}

/// The book in the right pane.
#[derive(Debug, Clone)]
struct Selected {
    id: i64,
    /// The book's description, parsed for the markdown widget.
    description: Vec<markdown::Item>,
}

#[derive(Debug, Clone)]
enum Message {
    /// A click on a row.
    Select(i64),
    /// A click on a column header.
    Sort(Column),
    /// A change to the filter field.
    Filter(String),
    /// A click on a link in the description. It does nothing.
    LinkClicked,
}

fn main() -> iced::Result {
    // The library stays open for the life of the window. Its lock keeps a
    // CLI command from changing the library while the window shows it.
    let (_library, viewer) = match open() {
        Ok((library, viewer)) => (Some(library), viewer),
        Err(e) => (None, Viewer::OpenFailed(format!("{e:#}"))),
    };
    iced::application(move || viewer.clone(), update, view)
        .title("EpubSync")
        .window_size((1180.0, 760.0))
        .default_font(theme::SANS)
        .font(include_bytes!("../fonts/instrument-sans/InstrumentSans[wdth,wght].ttf").as_slice())
        .font(include_bytes!("../fonts/newsreader/Newsreader[opsz,wght].ttf").as_slice())
        .font(include_bytes!("../fonts/newsreader/Newsreader-Italic[opsz,wght].ttf").as_slice())
        .font(include_bytes!("../fonts/jetbrains-mono/JetBrainsMono[wght].ttf").as_slice())
        .style(|_viewer, theme| theme::window(theme))
        .run()
}

fn open() -> anyhow::Result<(Library, Viewer)> {
    let config = config::load()?;
    let library = Library::open(&config)?;
    let (books, progress) = read::lists(&library)?;
    let open = Open {
        folder: library.folder.clone(),
        books,
        progress,
        sort: Sort::default(),
        filter: String::new(),
        selected: None,
    };
    Ok((library, Viewer::Open(open)))
}

fn update(viewer: &mut Viewer, message: Message) {
    let Viewer::Open(open) = viewer else {
        return;
    };
    match message {
        Message::Select(id) => {
            // The description is parsed only when the book is shown.
            let html = open
                .books
                .iter()
                .find(|e| e.book.id == id)
                .and_then(|e| e.book.metadata.description.as_deref())
                .unwrap_or("");
            open.selected = Some(Selected {
                id,
                description: description::parse(html),
            });
        }
        Message::Sort(column) => {
            if open.sort.column == column {
                open.sort.descending = !open.sort.descending;
            } else {
                open.sort = Sort {
                    column,
                    descending: false,
                };
            }
        }
        Message::Filter(text) => open.filter = text,
        Message::LinkClicked => {}
    }
}

fn view(viewer: &Viewer) -> Element<'_, Message> {
    match viewer {
        Viewer::OpenFailed(error) => container(text(error)).padding(16).into(),
        Viewer::Open(open) => {
            let rows = table::order(open);
            let shown = open.selected.as_ref().and_then(|s| {
                let entry = open.books.iter().find(|e| e.book.id == s.id)?;
                Some((entry, s))
            });
            let main =
                row![table::view(open, &rows), book_pane(shown, &open.progress)].height(Fill);
            column![toolbar(open, rows.len()), main, status_bar(open)].into()
        }
    }
}

/// "1 book" or "23 books".
fn books(n: usize) -> String {
    if n == 1 {
        "1 book".to_string()
    } else {
        format!("{n} books")
    }
}

/// The toolbar: the title, the count, and the filter field. The count
/// reads "4 of 23 books" while the filter is set.
fn toolbar<'a>(open: &'a Open, shown: usize) -> Element<'a, Message> {
    let total = open.books.len();
    let count = if open.filter.trim().is_empty() {
        books(total)
    } else {
        format!("{shown} of {}", books(total))
    };
    let filter = text_input("Filter by title, author, or series", &open.filter)
        .on_input(Message::Filter)
        .width(300)
        .size(13)
        .padding([5, 10])
        .style(theme::filter);
    let bar = row![
        text("Library").size(14).font(SANS_SEMIBOLD),
        text(count).size(BODY).style(theme::text_color(|c| c.muted)),
        space().width(Fill),
        filter,
    ]
    .spacing(14)
    .align_y(Center)
    .height(46)
    .padding(padding::horizontal(14));
    column![bar, theme::hline()].into()
}

/// The status bar: the count, how many books are reading and finished,
/// and the library folder.
fn status_bar(open: &Open) -> Element<'_, Message> {
    let has_status = |status: i64| {
        open.books
            .iter()
            .filter(|e| {
                open.progress
                    .iter()
                    .any(|p| p.book_id == e.book.id && p.status == status)
            })
            .count()
    };
    let counts = format!("{} reading · {} finished", has_status(1), has_status(2));
    let bar = row![
        text(books(open.books.len()))
            .size(11.5)
            .style(theme::text_color(|c| c.muted)),
        text(counts)
            .size(11.5)
            .style(theme::text_color(|c| c.muted)),
        space().width(Fill),
        text(open.folder.display().to_string())
            .font(MONO)
            .size(11)
            .wrapping(text::Wrapping::None)
            .style(theme::text_color(|c| c.muted)),
    ]
    .spacing(18)
    .align_y(Center)
    .height(28)
    .padding(padding::horizontal(14));
    column![theme::hline(), bar].into()
}

/// The right pane: the selected book's metadata, its description, its
/// progress on each device, and its id and file path.
fn book_pane<'a>(
    shown: Option<(&'a Entry, &'a Selected)>,
    progress: &'a [ProgressRow],
) -> Element<'a, Message> {
    let Some((entry, selected)) = shown else {
        return space().into();
    };
    let book = &entry.book;
    let m = &book.metadata;
    let mut lines = column![text(&m.title).size(24)].spacing(8);
    lines = lines.push(text(format::authors(&m.authors)));
    if let Some(series) = &m.series {
        lines = lines.push(text(format::series_tag(series)));
    }
    if let Some(publisher) = &m.publisher {
        lines = lines.push(text(publisher));
    }
    lines = lines
        .push(markdown::view(&selected.description, Theme::Light).map(|_uri| Message::LinkClicked));
    for p in progress.iter().filter(|p| p.book_id == book.id) {
        lines = lines.push(text(progress_line(p)));
    }
    lines = lines.push(text(format!("{}  {}", book.id, entry.path.display())));
    scrollable(lines.padding(16)).width(360).into()
}

/// One device's progress: the serial, the percent, the status, and the
/// day last read, formatted as `epubsync list` prints it.
fn progress_line(p: &ProgressRow) -> String {
    let day = table::day_of(p).unwrap_or("");
    format!(
        "{}: {}% {} {day}",
        p.device_serial,
        p.percent,
        format::status(p.status)
    )
    .trim_end()
    .to_string()
}
