//! The library viewer: a window with the book list on the left and the
//! selected book on the right. It is read-only. The CLI imports, edits,
//! removes, and syncs.

mod description;
mod read;

use epubsync_core::config;
use epubsync_core::library::{Book, Library, ProgressRow};
use epubsync_core::metadata::{Series, format_series_number};
use iced::widget::{button, column, container, markdown, row, scrollable, text};
use iced::{Element, Fill, Length, Theme};

use crate::read::Entry;

/// The state of the viewer window: what it draws.
#[derive(Debug, Clone)]
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the two panes.
    Open {
        /// The books in id order.
        books: Vec<Entry>,
        /// Reading progress, one row per book per device.
        progress: Vec<ProgressRow>,
        /// The book in the right pane, if any.
        selected: Option<Selected>,
    },
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
    /// A click on a book in the list.
    Select(i64),
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
        .theme(theme)
        .run()
}

fn open() -> anyhow::Result<(Library, Viewer)> {
    let config = config::load()?;
    let library = Library::open(&config)?;
    let (books, progress) = read::lists(&library)?;
    Ok((
        library,
        Viewer::Open {
            books,
            progress,
            selected: None,
        },
    ))
}

fn update(viewer: &mut Viewer, message: Message) {
    let Viewer::Open {
        books, selected, ..
    } = viewer
    else {
        return;
    };
    match message {
        Message::Select(id) => {
            // The description is parsed only when the book is shown.
            let html = books
                .iter()
                .find(|e| e.book.id == id)
                .and_then(|e| e.book.metadata.description.as_deref())
                .unwrap_or("");
            *selected = Some(Selected {
                id,
                description: description::parse(html),
            });
        }
        Message::LinkClicked => {}
    }
}

fn view(viewer: &Viewer) -> Element<'_, Message> {
    match viewer {
        Viewer::OpenFailed(error) => container(text(error)).padding(16).into(),
        Viewer::Open {
            books,
            progress,
            selected,
        } => {
            let shown = selected.as_ref().and_then(|s| {
                let entry = books.iter().find(|e| e.book.id == s.id)?;
                Some((entry, s))
            });
            let selected_id = selected.as_ref().map(|s| s.id);
            row![book_list(books, selected_id), book_pane(shown, progress)].into()
        }
    }
}

/// The theme is fixed so that the description's Markdown, which takes a
/// theme when it is drawn, matches the rest of the window.
fn theme(_viewer: &Viewer) -> Theme {
    Theme::Light
}

/// The left pane: one button per book. The selected book's button is
/// filled; the others are plain text.
fn book_list(books: &[Entry], selected_id: Option<i64>) -> Element<'_, Message> {
    let buttons = books.iter().map(|entry| {
        let book = &entry.book;
        let label = column![text(&book.metadata.title), text(byline(book))].spacing(2);
        let style = if Some(book.id) == selected_id {
            button::primary
        } else {
            button::text
        };
        button(label)
            .on_press(Message::Select(book.id))
            .width(Fill)
            .style(style)
            .into()
    });
    scrollable(column(buttons).spacing(2).padding(8))
        .width(Length::FillPortion(2))
        .into()
}

/// The right pane: the selected book's metadata, its description, its
/// progress on each device, and its id and file path.
fn book_pane<'a>(
    shown: Option<(&'a Entry, &'a Selected)>,
    progress: &'a [ProgressRow],
) -> Element<'a, Message> {
    let Some((entry, selected)) = shown else {
        return container(text("Select a book"))
            .width(Length::FillPortion(3))
            .padding(16)
            .into();
    };
    let book = &entry.book;
    let m = &book.metadata;
    let mut lines = column![text(&m.title).size(24)].spacing(8);
    lines = lines.push(text(authors(book)));
    if let Some(series) = &m.series {
        lines = lines.push(text(series_tag(series)));
    }
    if let Some(publisher) = &m.publisher {
        lines = lines.push(text(publisher));
    }
    lines = lines
        .push(markdown::view(&selected.description, Theme::Light).map(|_uri| Message::LinkClicked));
    for p in progress.iter().filter(|p| p.book_id == book.id) {
        lines = lines.push(text(progress_cell(p)));
    }
    lines = lines.push(text(format!("{}  {}", book.id, entry.path.display())));
    scrollable(lines.padding(16))
        .width(Length::FillPortion(3))
        .into()
}

/// The authors joined with " & ".
fn authors(book: &Book) -> String {
    let authors: Vec<&str> = book
        .metadata
        .authors
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    authors.join(" & ")
}

/// The authors, then the series in square brackets, the way
/// `epubsync list` prints them.
fn byline(book: &Book) -> String {
    let mut line = authors(book);
    if let Some(series) = &book.metadata.series {
        line.push_str("  ");
        line.push_str(&series_tag(series));
    }
    line
}

/// The series name and number as "[Name #1]".
fn series_tag(series: &Series) -> String {
    match series.number {
        Some(n) => format!("[{} #{}]", series.name, format_series_number(n)),
        None => format!("[{}]", series.name),
    }
}

/// One device's progress: the serial, the percent, the status, and the
/// day last read, formatted as `epubsync list` prints it.
fn progress_cell(p: &ProgressRow) -> String {
    let status = match p.status {
        0 => "unread",
        1 => "reading",
        2 => "finished",
        _ => "status ?",
    };
    let day = p
        .last_read
        .as_deref()
        .map(|d| &d[..d.len().min(10)])
        .unwrap_or("");
    format!("{}: {}% {status} {day}", p.device_serial, p.percent)
        .trim_end()
        .to_string()
}
