//! The library viewer: a window with a table of books and, when a book is
//! selected, its details in a sidebar on the right. A Words tab swaps the
//! table for the list of words looked up on a device. It is read-only.
//! The CLI imports, edits, removes, and syncs.

mod description;
mod detail;
mod format;
mod table;
mod theme;
mod words;

use std::collections::BTreeMap;
use std::path::PathBuf;

use epubsync_core::config;
use epubsync_core::device::ReadStatus;
use epubsync_core::library::{Book, Library, ProgressRow, WordRow};
use epubsync_core::query::{self, Query, Sort, SortKey};
use iced::keyboard::{self, key};
use iced::widget::{button, column, container, markdown, row, space, text, text_input};
use iced::{Center, Element, Fill, Subscription, Task, padding};

use crate::theme::{BODY, MONO, SANS_SEMIBOLD};

/// The state of the viewer window: what it draws.
#[derive(Debug, Clone)]
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the table and, when a book
    /// is selected, the sidebar. The box keeps the enum the size of the
    /// small variant.
    Open(Box<Open>),
}

#[derive(Debug, Clone)]
struct Open {
    /// The library folder. The status bar shows it.
    folder: PathBuf,
    /// The books in id order. The table sorts a borrowed view.
    books: Vec<Book>,
    /// Reading progress by book id, one row per device.
    progress: BTreeMap<i64, Vec<ProgressRow>>,
    /// Every looked-up word, newest first. The words pane lists them all
    /// and the sidebar lists the selected book's.
    words: Vec<WordRow>,
    /// The pane in the main area.
    pane: Pane,
    /// The sorted column and the filter field's text, as the core query
    /// the table selects its rows with. The words pane matches the same
    /// text against the word and the book title.
    query: Query,
    /// The scroll offset of the pane in view, in pixels. The pane builds
    /// only the rows in view at that offset.
    scroll: f32,
    /// The book in the sidebar, if any.
    selected: Option<Selected>,
}

/// The pane in the main area: the table of books, or the list of words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Books,
    Words,
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
    /// A click on a toolbar tab.
    Show(Pane),
    /// A click on a column header.
    Sort(SortKey),
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
    let config = config::load(&config::path()?)?;
    let library = Library::open(&config)?;
    let open = Open {
        folder: library.folder.clone(),
        books: library.list()?,
        progress: library.progress()?,
        words: library.words(None, None)?,
        pane: Pane::Books,
        query: Query::default(),
        scroll: 0.0,
        selected: None,
    };
    Ok((library, Viewer::Open(Box::new(open))))
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
        Message::Show(pane) => {
            // The two panes share one scrollable id, so the new pane
            // starts at the top rather than at the old pane's offset.
            if open.pane != pane {
                open.pane = pane;
                open.scroll = 0.0;
                return table::scroll_to_top();
            }
        }
        Message::Sort(key) => {
            let sort = &mut open.query.sort;
            if sort.keys == [key] {
                sort.descending = !sort.descending;
            } else {
                *sort = Sort::by(key);
            }
        }
        Message::Filter(text) => {
            // A new filter shows its matches from the top.
            open.query.filter.text = text;
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
            let (pane, shown_count): (Element<'_, Message>, usize) = match open.pane {
                Pane::Books => {
                    let rows = open.query.select(&open.books, &open.progress);
                    let n = rows.len();
                    (table::view(open, rows), n)
                }
                Pane::Words => {
                    let rows = words::select(&open.words, &open.query.filter.text);
                    let n = rows.len();
                    (words::view(open, rows), n)
                }
            };
            let shown = open.selected.as_ref().and_then(|s| {
                let book = open.books.iter().find(|b| b.id == s.id)?;
                Some((book, s))
            });
            let mut main = row![pane].height(Fill);
            if let Some((book, selected)) = shown {
                main = main.push(detail::view(open, book, selected));
            }
            column![toolbar(open, shown_count), main, status_bar(open)].into()
        }
    }
}

/// "1 book", "23 books", "1 word", "12 words".
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The toolbar: the Library and Words tabs, the count for the pane in
/// view, and the filter field. The count reads "4 of 23 books" while
/// the filter is set.
fn toolbar<'a>(open: &'a Open, shown: usize) -> Element<'a, Message> {
    let (total, noun, placeholder) = match open.pane {
        Pane::Books => (
            open.books.len(),
            "book",
            "Filter by title, author, or series",
        ),
        Pane::Words => (open.words.len(), "word", "Filter by word or book"),
    };
    let count = if open.query.filter.is_empty() {
        count(total, noun)
    } else {
        format!("{shown} of {}", count(total, noun))
    };
    let tab = |label: &'static str, pane: Pane| {
        button(text(label).size(14).font(SANS_SEMIBOLD))
            .on_press(Message::Show(pane))
            .padding(0)
            .style(theme::tab(open.pane == pane))
    };
    let filter = text_input(placeholder, &open.query.filter.text)
        .on_input(Message::Filter)
        .width(300)
        .size(13)
        .padding([5, 10])
        .style(theme::filter);
    let bar = row![
        tab("Library", Pane::Books),
        tab("Words", Pane::Words),
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

/// The status bar: the count, how many books are reading and finished by
/// the progress row read last, how many were finished this year by
/// their finished date, and the library folder.
fn status_bar(open: &Open) -> Element<'_, Message> {
    let has_status = |status: ReadStatus| {
        open.books
            .iter()
            .filter(|b| query::status(&open.progress, b.id) == status)
            .count()
    };
    let year = format::this_year();
    let this_year = open
        .books
        .iter()
        .filter(|b| query::finished(&open.progress, b.id).is_some_and(|d| d.starts_with(&year)))
        .count();
    let counts = format!(
        "{} reading · {} finished · {this_year} this year",
        has_status(ReadStatus::Reading),
        has_status(ReadStatus::Finished)
    );
    let bar = row![
        text(count(open.books.len(), "book"))
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
