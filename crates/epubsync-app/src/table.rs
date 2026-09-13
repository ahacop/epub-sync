//! The table of books: the columns, the sort and filter logic, and the
//! table view.

use std::cmp::Ordering;

use epubsync_core::library::ProgressRow;
use iced::widget::{Text, button, column, container, progress_bar, row, scrollable, space, text};
use iced::{Center, Element, Fill, Length, Right, padding};

use crate::read::Entry;
use crate::theme::{self, BODY, MONO, SANS_MEDIUM};
use crate::{Message, Open, format};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Id,
    Title,
    Author,
    Series,
    Progress,
    LastRead,
}

impl Column {
    /// The columns from left to right.
    pub const ALL: [Column; 6] = [
        Column::Id,
        Column::Title,
        Column::Author,
        Column::Series,
        Column::Progress,
        Column::LastRead,
    ];

    fn name(self) -> &'static str {
        match self {
            Column::Id => "ID",
            Column::Title => "Title",
            Column::Author => "Author",
            Column::Series => "Series",
            Column::Progress => "Progress",
            Column::LastRead => "Last read",
        }
    }

    /// The fixed columns take pixels; the text columns share the rest.
    fn width(self) -> Length {
        match self {
            Column::Id => Length::Fixed(56.0),
            Column::Title => Length::FillPortion(32),
            Column::Author => Length::FillPortion(20),
            Column::Series => Length::FillPortion(18),
            Column::Progress => Length::Fixed(200.0),
            Column::LastRead => Length::Fixed(108.0),
        }
    }
}

/// The sorted column and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort {
            column: Column::Title,
            descending: false,
        }
    }
}

/// The width of the selected row's mark on the left edge. Every row and
/// the header row leave this space, so the cells line up.
const MARK: f32 = 3.0;

/// The progress row with the greatest `last_read` for a book. A row
/// without `last_read` counts as the oldest.
pub fn latest(progress: &[ProgressRow], book_id: i64) -> Option<&ProgressRow> {
    progress
        .iter()
        .filter(|p| p.book_id == book_id)
        .max_by_key(|p| p.last_read.as_deref())
}

/// The day part of a progress row's `last_read`.
pub fn day_of(p: &ProgressRow) -> Option<&str> {
    p.last_read.as_deref().map(|d| d.get(..10).unwrap_or(d))
}

/// The sort key of one book for one column. `None` means the book has no
/// value in that column, and sorts after every book that has one.
#[derive(Debug, PartialEq, PartialOrd)]
enum Key {
    Id(i64),
    Title(String),
    Author(String),
    Series(String, Option<f64>),
    Percent(i64),
    Day(String),
}

fn key(entry: &Entry, column: Column, progress: &[ProgressRow]) -> Option<Key> {
    let book = &entry.book;
    let m = &book.metadata;
    match column {
        Column::Id => Some(Key::Id(book.id)),
        Column::Title => Some(Key::Title(format::title_key(&m.title))),
        Column::Author => m
            .authors
            .first()
            .map(|a| Key::Author(a.sort.to_lowercase())),
        Column::Series => m
            .series
            .as_ref()
            .map(|s| Key::Series(s.name.to_lowercase(), s.number)),
        Column::Progress => latest(progress, book.id).map(|p| Key::Percent(p.percent)),
        Column::LastRead => latest(progress, book.id)
            .and_then(day_of)
            .map(|d| Key::Day(d.to_string())),
    }
}

/// Orders two keys. A missing key comes after every present key.
fn compare(a: &Option<Key>, b: &Option<Key>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Whether the filter text is in the title, an author name, or the
/// series name. `needle` is already trimmed and in lower case.
fn matches(entry: &Entry, needle: &str) -> bool {
    let m = &entry.book.metadata;
    m.title.to_lowercase().contains(needle)
        || m.authors
            .iter()
            .any(|a| a.name.to_lowercase().contains(needle))
        || m.series
            .as_ref()
            .is_some_and(|s| s.name.to_lowercase().contains(needle))
}

/// The books the table shows, filtered and sorted. Ties keep id order.
pub fn order(open: &Open) -> Vec<&Entry> {
    let needle = open.filter.trim().to_lowercase();
    let mut rows: Vec<(Option<Key>, &Entry)> = open
        .books
        .iter()
        .filter(|e| needle.is_empty() || matches(e, &needle))
        .map(|e| (key(e, open.sort.column, &open.progress), e))
        .collect();
    // The sort is stable, and the books are in id order.
    rows.sort_by(|(a, _), (b, _)| {
        let order = compare(a, b);
        if open.sort.descending {
            order.reverse()
        } else {
            order
        }
    });
    rows.into_iter().map(|(_, e)| e).collect()
}

/// The table: the header row, then the rows in a scrollable column.
pub fn view<'a>(open: &'a Open, rows: &[&'a Entry]) -> Element<'a, Message> {
    let selected = open.selected.as_ref().map(|s| s.id);
    let mut headers = row![space().width(MARK)].height(32).align_y(Center);
    for column in Column::ALL {
        headers = headers.push(header(column, open.sort));
    }
    let body = column(
        rows.iter()
            .map(|e| book_row(e, &open.progress, selected == Some(e.book.id))),
    );
    let table = column![
        container(headers).style(theme::ground(|c| c.window)),
        theme::hline(),
        scrollable(body).width(Fill).height(Fill),
    ];
    container(table)
        .width(Fill)
        .height(Fill)
        .style(theme::ground(|c| c.surface))
        .into()
}

/// A column header: the name, and an arrow on the sorted column.
fn header<'a>(column: Column, sort: Sort) -> Element<'a, Message> {
    let sorted = sort.column == column;
    let color: fn(&theme::Colors) -> iced::Color = if sorted { |c| c.ink } else { |c| c.muted };
    let name = text(column.name()).size(12).style(theme::text_color(color));
    let mut label = row![name].spacing(3).align_y(Center);
    if sorted {
        let arrow = if sort.descending { "▼" } else { "▲" };
        label = label.push(text(arrow).size(9).style(theme::text_color(|c| c.accent)));
    }
    button(cell(label, column))
        .on_press(Message::Sort(column))
        .height(Fill)
        .padding(0)
        .style(theme::header)
        .into()
}

/// One row: a button with a cell per column and a 1 px line under it.
fn book_row<'a>(
    entry: &'a Entry,
    progress: &'a [ProgressRow],
    selected: bool,
) -> Element<'a, Message> {
    let book = &entry.book;
    let m = &book.metadata;
    let latest = latest(progress, book.id);

    let mark = container(space()).width(MARK).height(Fill);
    let mark = if selected {
        mark.style(theme::ground(|c| c.accent))
    } else {
        mark
    };
    let id = line(book.id)
        .font(MONO)
        .size(12)
        .style(theme::text_color(|c| c.muted));
    let title = line(&m.title).font(SANS_MEDIUM);
    let author = line(format::authors(&m.authors)).style(theme::text_color(|c| c.muted));
    let series: Element<'a, Message> = match &m.series {
        Some(s) => {
            let name = line(&s.name).style(theme::text_color(|c| c.muted));
            let mut parts = row![name].spacing(4);
            if let Some(n) = s.number {
                let number = format!("#{}", epubsync_core::metadata::format_series_number(n));
                parts = parts.push(line(number).style(theme::text_color(|c| c.faint)));
            }
            parts.into()
        }
        None => space().into(),
    };
    let last_read = line(latest.and_then(day_of).map(format::day).unwrap_or_default())
        .style(theme::text_color(|c| c.muted));

    let cells = row![
        mark,
        cell(id, Column::Id),
        cell(title, Column::Title),
        cell(author, Column::Author),
        cell(series, Column::Series),
        cell(progress_cell(latest), Column::Progress),
        cell(last_read, Column::LastRead),
    ]
    .height(Fill)
    .align_y(Center);
    column![
        button(cells)
            .on_press(Message::Select(book.id))
            .width(Fill)
            .height(34)
            .padding(0)
            .style(theme::row(selected)),
        theme::hline(),
    ]
    .into()
}

/// Cell text at the body size on one line. The cell clips what does not
/// fit.
fn line<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(BODY).wrapping(text::Wrapping::None)
}

/// A cell: the column's width, 12 px side padding, one line, clipped.
/// The id column is right-aligned.
fn cell<'a>(content: impl Into<Element<'a, Message>>, column: Column) -> Element<'a, Message> {
    let mut cell = container(content)
        .width(column.width())
        .padding(padding::horizontal(12))
        .clip(true);
    if column == Column::Id {
        cell = cell.align_x(Right);
    }
    cell.into()
}

/// The latest progress: a bar, the percent, and the status chip. A book
/// with no progress row shows a dash.
fn progress_cell<'a>(latest: Option<&'a ProgressRow>) -> Element<'a, Message> {
    let Some(p) = latest else {
        return text("—")
            .size(BODY)
            .style(theme::text_color(|c| c.faint))
            .into();
    };
    row![
        progress_bar(0.0..=100.0, p.percent as f32)
            .length(64)
            .girth(4)
            .style(theme::bar(p.status)),
        line(format!("{}%", p.percent))
            .width(34)
            .style(theme::text_color(|c| c.ink_2)),
        chip(p.status),
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

/// The status word in upper case on its tint.
pub fn chip<'a>(status: i64) -> Element<'a, Message> {
    container(
        text(format::status(status).to_uppercase())
            .size(11)
            .font(SANS_MEDIUM),
    )
    .padding([1, 6])
    .style(theme::chip(status))
    .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use epubsync_core::library::{Book, ProgressRow};
    use epubsync_core::metadata::{Author, Metadata, Series};

    use super::*;

    fn author(name: &str, sort: &str) -> Author {
        Author {
            name: name.into(),
            sort: sort.into(),
        }
    }

    fn entry(id: i64, title: &str, author: Option<Author>, series: Option<Series>) -> Entry {
        Entry {
            book: Book {
                id,
                revision: 1,
                metadata: Metadata {
                    title: title.into(),
                    authors: author.into_iter().collect(),
                    series,
                    ..Metadata::default()
                },
            },
            path: PathBuf::from(format!("{id}.kepub.epub")),
        }
    }

    fn progress(book_id: i64, percent: i64, status: i64, last_read: Option<&str>) -> ProgressRow {
        ProgressRow {
            book_id,
            device_serial: "N123".into(),
            percent,
            status,
            last_read: last_read.map(|d| format!("{d}T10:00:00Z")),
        }
    }

    /// Three books: The Warden (Trollope, Barsetshire 1, 62% reading),
    /// Villette (Brontë, no series, no progress), and A Princess of Mars
    /// (Burroughs, Martian 1, 100% finished).
    fn library() -> Open {
        Open {
            folder: PathBuf::from("/books"),
            books: vec![
                entry(
                    1,
                    "The Warden",
                    Some(author("Anthony Trollope", "Trollope, Anthony")),
                    Some(Series {
                        name: "Chronicles of Barsetshire".into(),
                        number: Some(1.0),
                    }),
                ),
                entry(
                    2,
                    "Villette",
                    Some(author("Charlotte Brontë", "Brontë, Charlotte")),
                    None,
                ),
                entry(
                    3,
                    "A Princess of Mars",
                    Some(author("Edgar Rice Burroughs", "Burroughs, Edgar Rice")),
                    Some(Series {
                        name: "Martian".into(),
                        number: Some(1.0),
                    }),
                ),
            ],
            progress: vec![
                progress(1, 62, 1, Some("2026-09-08")),
                progress(3, 100, 2, Some("2026-05-12")),
            ],
            sort: Sort::default(),
            filter: String::new(),
            selected: None,
        }
    }

    fn ids(open: &Open) -> Vec<i64> {
        order(open).iter().map(|e| e.book.id).collect()
    }

    #[test]
    fn default_order_is_by_title_without_articles() {
        let open = library();
        // princess of mars, villette, warden
        assert_eq!(ids(&open), vec![3, 2, 1]);
    }

    #[test]
    fn descending_flips_the_order() {
        let mut open = library();
        open.sort.descending = true;
        assert_eq!(ids(&open), vec![1, 2, 3]);
    }

    #[test]
    fn author_order_uses_the_sort_name() {
        let mut open = library();
        open.sort.column = Column::Author;
        // Brontë, Burroughs, Trollope
        assert_eq!(ids(&open), vec![2, 3, 1]);
    }

    #[test]
    fn series_order_puts_a_book_without_a_series_last() {
        let mut open = library();
        open.sort.column = Column::Series;
        // Chronicles of Barsetshire, Martian, then Villette
        assert_eq!(ids(&open), vec![1, 3, 2]);
    }

    #[test]
    fn progress_order_puts_a_book_without_a_row_last() {
        let mut open = library();
        open.sort.column = Column::Progress;
        // 62%, 100%, then Villette
        assert_eq!(ids(&open), vec![1, 3, 2]);
    }

    #[test]
    fn last_read_order_puts_a_book_without_a_row_last() {
        let mut open = library();
        open.sort.column = Column::LastRead;
        // May, September, then Villette
        assert_eq!(ids(&open), vec![3, 1, 2]);
    }

    #[test]
    fn latest_row_is_the_one_read_last() {
        let rows = vec![
            progress(1, 10, 1, None),
            progress(1, 62, 1, Some("2026-09-08")),
            progress(1, 30, 1, Some("2026-07-02")),
        ];
        assert_eq!(latest(&rows, 1).map(|p| p.percent), Some(62));
        assert_eq!(latest(&rows, 2), None);
    }

    #[test]
    fn filter_matches_an_author_name() {
        let mut open = library();
        open.filter = "bront".into();
        assert_eq!(ids(&open), vec![2]);
        open.filter = "  BURROUGHS ".into();
        assert_eq!(ids(&open), vec![3]);
    }

    #[test]
    fn filter_matches_a_series_name() {
        let mut open = library();
        open.filter = "barset".into();
        assert_eq!(ids(&open), vec![1]);
    }
}
