//! The library viewer: a window with the book list on the left and the
//! selected book on the right. It is read-only. The CLI imports, edits,
//! removes, and syncs.

mod read;

use epubsync_core::config;
use epubsync_core::library::{Book, Library};
use epubsync_core::metadata::{Series, format_series_number};
use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Fill, Theme};

/// The state of the viewer window: what it draws.
#[derive(Debug, Clone)]
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the book list.
    Open {
        /// The books in id order.
        books: Vec<Book>,
    },
}

#[derive(Debug, Clone)]
enum Message {}

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
    let books = read::books(&library)?;
    Ok((library, Viewer::Open { books }))
}

fn update(_viewer: &mut Viewer, message: Message) {
    match message {}
}

fn view(viewer: &Viewer) -> Element<'_, Message> {
    match viewer {
        Viewer::OpenFailed(error) => container(text(error)).padding(16).into(),
        Viewer::Open { books } => book_list(books),
    }
}

/// The theme is fixed so that the description's Markdown, which takes a
/// theme when it is drawn, matches the rest of the window.
fn theme(_viewer: &Viewer) -> Theme {
    Theme::Light
}

/// The left pane: one button per book.
fn book_list(books: &[Book]) -> Element<'_, Message> {
    let buttons = books.iter().map(|book| {
        let label = column![text(&book.metadata.title), text(byline(book))].spacing(2);
        button(label).width(Fill).style(button::text).into()
    });
    scrollable(column(buttons).spacing(2).padding(8))
        .width(Fill)
        .into()
}

/// The authors joined with " & ", then the series in square brackets, the
/// way `epubsync list` prints them.
fn byline(book: &Book) -> String {
    let m = &book.metadata;
    let authors: Vec<&str> = m.authors.iter().map(|a| a.name.as_str()).collect();
    let mut line = authors.join(" & ");
    if let Some(series) = &m.series {
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
