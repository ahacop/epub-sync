//! The library viewer: a window with a table of books and, when a book is
//! selected, its details in a sidebar on the right. A Words tab swaps the
//! table for the list of words looked up on a device. It reads the
//! library and never writes it. The CLI imports, edits, removes, and
//! syncs. A Reload button reads the library again.

mod description;
mod detail;
mod format;
mod table;
mod theme;
mod words;

use std::collections::BTreeMap;

use epubsync_core::config;
use epubsync_core::device::ReadStatus;
use epubsync_core::library::{Book, Library, ProgressRow, WordRow};
use epubsync_core::query::{self, Query, Sort, SortKey};
use iced::keyboard::{self, key};
use iced::widget::{button, column, container, markdown, row, space, text, text_input};
use iced::{Center, Element, Fill, Subscription, Task, padding};

use crate::theme::{BODY, MONO, SANS_SEMIBOLD};

/// The state of the viewer window: what it draws.
enum Viewer {
    /// The library could not be opened. The window shows the error text.
    OpenFailed(String),
    /// The library is open. The window shows the table and, when a book
    /// is selected, the sidebar. The box keeps the enum the size of the
    /// small variant.
    Open(Box<Open>),
}

struct Open {
    /// The open library. It holds the lock for the life of the window,
    /// so a CLI command fails while the window shows the library.
    library: Library,
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
    /// Why the last reload failed, if it did. The status bar shows it
    /// until a reload succeeds.
    error: Option<String>,
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
    /// The Reload button. The viewer reads the library again.
    Reload,
    /// A click on a link in the description. It does nothing.
    LinkClicked,
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
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

/// The state at start: the open library, or the reason it did not open.
fn boot() -> Viewer {
    match open() {
        Ok(open) => Viewer::Open(Box::new(open)),
        Err(e) => Viewer::OpenFailed(format!("{e:#}")),
    }
}

fn open() -> anyhow::Result<Open> {
    let config = config::load(&config::path()?)?;
    let library = Library::open(&config)?;
    Open::new(library)
}

impl Open {
    /// The state for an open library, with its books, progress, and
    /// words read once.
    fn new(library: Library) -> anyhow::Result<Open> {
        let mut open = Open {
            library,
            books: Vec::new(),
            progress: BTreeMap::new(),
            words: Vec::new(),
            pane: Pane::Books,
            query: Query::default(),
            scroll: 0.0,
            selected: None,
            error: None,
        };
        open.read()?;
        Ok(open)
    }

    /// Reads the books, the progress, and the words from the library.
    /// The state changes only when all three reads succeed.
    fn read(&mut self) -> anyhow::Result<()> {
        let books = self.library.list()?;
        let progress = self.library.progress()?;
        let words = self.library.words(None, None)?;
        self.books = books;
        self.progress = progress;
        self.words = words;
        Ok(())
    }

    /// Reads the library again. The sort, the filter, and the scroll
    /// offset stay. The sidebar stays on its book with the description
    /// parsed again, and closes when the book is gone. A read that
    /// fails keeps the rows from the last read and puts the error in
    /// the status bar.
    ///
    /// The viewer holds the library lock, so no other process writes
    /// while the window is open. A write from the viewer ends with a
    /// reload.
    fn reload(&mut self) {
        self.error = self.read().err().map(|e| format!("{e:#}"));
        if let Some(id) = self.selected.as_ref().map(|s| s.id) {
            self.select(id);
        }
    }

    /// Puts the book in the sidebar. Its description is parsed here, so
    /// a book that is never shown is never parsed. An id no book has
    /// closes the sidebar.
    fn select(&mut self, id: i64) {
        self.selected = self.books.iter().find(|b| b.id == id).map(|b| {
            let html = b.metadata.description.as_deref().unwrap_or("");
            Selected {
                id,
                description: description::parse(html),
            }
        });
    }
}

fn update(viewer: &mut Viewer, message: Message) -> Task<Message> {
    let Viewer::Open(open) = viewer else {
        return Task::none();
    };
    match message {
        Message::Select(id) => open.select(id),
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
        Message::Reload => open.reload(),
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
/// view, the filter field, and the Reload button. The count reads
/// "4 of 23 books" while the filter is set.
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
    let reload = button(text("Reload").size(13))
        .on_press(Message::Reload)
        .padding([5, 10])
        .style(theme::action);
    let bar = row![
        tab("Library", Pane::Books),
        tab("Words", Pane::Words),
        text(count).size(BODY).style(theme::text_color(|c| c.muted)),
        space().width(Fill),
        filter,
        reload,
    ]
    .spacing(14)
    .align_y(Center)
    .height(46)
    .padding(padding::horizontal(14));
    column![bar, theme::hline()].into()
}

/// The status bar: the count, how many books are reading and finished by
/// the progress row read last, how many were finished this year by
/// their finished date, and the library folder. The error of a failed
/// reload takes the place of the counts.
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
    let counts = match &open.error {
        Some(error) => text(format!("Reload failed: {error}"))
            .size(11.5)
            .style(theme::text_color(|c| c.ink)),
        None => text(format!(
            "{} reading · {} finished · {this_year} this year",
            has_status(ReadStatus::Reading),
            has_status(ReadStatus::Finished)
        ))
        .size(11.5)
        .style(theme::text_color(|c| c.muted)),
    };
    let bar = row![
        text(count(open.books.len(), "book"))
            .size(11.5)
            .style(theme::text_color(|c| c.muted)),
        counts,
        space().width(Fill),
        text(open.library.folder.display().to_string())
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
