//! The library viewer: a window with a table of books and, when a book is
//! selected, its details in a sidebar on the right. It is read-only. The
//! CLI imports, edits, removes, and syncs.

mod description;
mod detail;
mod format;
mod table;
mod theme;

use std::collections::BTreeMap;
use std::path::PathBuf;

use epubsync_core::config;
use epubsync_core::device::ReadStatus;
use epubsync_core::library::{Book, Library, ProgressRow};
use iced::keyboard::{self, key};
use iced::widget::{column, container, markdown, row, space, text, text_input};
use iced::{Center, Element, Fill, Subscription, Task, padding};

use crate::table::{Column, Sort};
use crate::theme::{BODY, MONO, SANS_SEMIBOLD};

/// The state of the viewer window: what it draws.
#[derive(Debug, Clone)]
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the table and, when a book
    /// is selected, the sidebar.
    Open(Open),
}

#[derive(Debug, Clone)]
struct Open {
    /// The library folder. The status bar shows it.
    folder: PathBuf,
    /// The books in id order. The table sorts a borrowed view.
    books: Vec<Book>,
    /// Reading progress by book id, one row per device.
    progress: BTreeMap<i64, Vec<ProgressRow>>,
    sort: Sort,
    /// The filter field's text.
    filter: String,
    /// The table body's scroll offset in pixels. The table builds only
    /// the rows in view at that offset.
    scroll: f32,
    /// The book in the sidebar, if any.
    selected: Option<Selected>,
}

/// The book in the sidebar.
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
    /// The sidebar's close button, or the Escape key.
    Close,
    /// A click on a column header.
    Sort(Column),
    /// A change to the filter field.
    Filter(String),
    /// The table body scrolled to this offset in pixels.
    Scrolled(f32),
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
        .subscription(subscription)
        .run()
}

/// Escape closes the sidebar. The filter field takes Escape first while
/// it has focus, to drop the focus.
fn subscription(_viewer: &Viewer) -> Subscription<Message> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Escape),
            ..
        } => Some(Message::Close),
        _ => None,
    })
}

fn open() -> anyhow::Result<(Library, Viewer)> {
    let config = config::load()?;
    let library = Library::open(&config)?;
    let open = Open {
        folder: library.folder.clone(),
        books: library.list()?,
        progress: library.progress()?,
        sort: Sort::default(),
        filter: String::new(),
        scroll: 0.0,
        selected: None,
    };
    Ok((library, Viewer::Open(open)))
}

fn update(viewer: &mut Viewer, message: Message) -> Task<Message> {
    let Viewer::Open(open) = viewer else {
        return Task::none();
    };
    match message {
        Message::Select(id) => {
            // The description is parsed only when the book is shown.
            let html = open
                .books
                .iter()
                .find(|b| b.id == id)
                .and_then(|b| b.metadata.description.as_deref())
                .unwrap_or("");
            open.selected = Some(Selected {
                id,
                description: description::parse(html),
            });
        }
        Message::Close => open.selected = None,
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
        Message::Filter(text) => {
            // A new filter shows its matches from the top.
            open.filter = text;
            open.scroll = 0.0;
            return table::scroll_to_top();
        }
        Message::Scrolled(offset) => open.scroll = offset,
        Message::LinkClicked => {}
    }
    Task::none()
}

fn view(viewer: &Viewer) -> Element<'_, Message> {
    match viewer {
        Viewer::OpenFailed(error) => container(text(error)).padding(16).into(),
        Viewer::Open(open) => {
            let rows = table::order(open);
            let shown = open.selected.as_ref().and_then(|s| {
                let book = open.books.iter().find(|b| b.id == s.id)?;
                Some((book, s))
            });
            let shown_count = rows.len();
            let mut main = row![table::view(open, rows)].height(Fill);
            if let Some((book, selected)) = shown {
                main = main.push(detail::view(open, book, selected));
            }
            column![toolbar(open, shown_count), main, status_bar(open)].into()
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
    let has_status = |status: ReadStatus| {
        open.books
            .iter()
            .filter(|b| {
                open.progress
                    .get(&b.id)
                    .is_some_and(|rows| rows.iter().any(|p| p.status == status))
            })
            .count()
    };
    let counts = format!(
        "{} reading · {} finished",
        has_status(ReadStatus::Reading),
        has_status(ReadStatus::Finished)
    );
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
