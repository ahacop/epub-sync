//! The words pane: every word looked up on a device, newest first, with
//! the book it came from, the device, and the day. It stands in for the
//! table while the Words tab is selected.

use epubsync_core::library::{Book, WordRow};
use iced::widget::{column, container, responsive, row, text};
use iced::{Center, Element, Fill, Length, Size};

use crate::theme::{self, MONO, SANS_MEDIUM};
use crate::{Message, Open, format, table};

/// The columns from left to right.
#[derive(Debug, Clone, Copy)]
enum Column {
    Word,
    Book,
    Device,
    Day,
}

const COLUMNS: [Column; 4] = [Column::Word, Column::Book, Column::Device, Column::Day];

fn name(column: Column) -> &'static str {
    match column {
        Column::Word => "Word",
        Column::Book => "Book",
        Column::Device => "Device",
        Column::Day => "Looked up",
    }
}

/// The fixed columns take pixels; the text columns share the rest.
fn width(column: Column) -> Length {
    match column {
        Column::Word => Length::FillPortion(30),
        Column::Book => Length::FillPortion(50),
        Column::Device => Length::Fixed(136.0),
        Column::Day => Length::Fixed(108.0),
    }
}

/// The title to show for a word: the library's title when the word came
/// from a library book, else the title the device had for it.
pub fn book_title<'a>(books: &'a [Book], word: &'a WordRow) -> Option<&'a str> {
    let in_library = word
        .book_id
        .and_then(|id| books.iter().find(|b| b.id == id))
        .map(|b| b.metadata.title.as_str());
    in_library.or(word.book_title.as_deref())
}

/// The words that match the filter text, in the order given. The match is
/// a case-insensitive substring of the word or of the book title.
pub fn select<'a>(words: &'a [WordRow], books: &[Book], filter: &str) -> Vec<&'a WordRow> {
    let needle = filter.trim().to_lowercase();
    words
        .iter()
        .filter(|w| {
            needle.is_empty()
                || w.word.to_lowercase().contains(&needle)
                || book_title(books, w).is_some_and(|t| t.to_lowercase().contains(&needle))
        })
        .collect()
}

/// The pane: the header row, then the rows in a scrollable column. A
/// library with no words at all shows a note in place of the rows.
pub fn view<'a>(open: &'a Open, rows: Vec<&'a WordRow>) -> Element<'a, Message> {
    let mut headers = row![].height(theme::HEADER).align_y(Center);
    for column in COLUMNS {
        let label = theme::label(name(column)).style(theme::text_color(|c| c.muted));
        headers = headers.push(table::cell(label, width(column)));
    }
    let body: Element<'a, Message> = if open.words.is_empty() {
        container(
            text("No words yet. Sync reads the words looked up on the Kobo.")
                .size(13)
                .style(theme::text_color(|c| c.faint)),
        )
        .padding(20)
        .into()
    } else {
        responsive(move |size| body(open, &rows, size)).into()
    };
    let pane = column![
        container(headers).style(theme::ground(|c| c.window)),
        theme::hline(),
        body,
    ];
    container(pane)
        .width(Fill)
        .height(Fill)
        .style(theme::ground(|c| c.surface))
        .into()
}

fn body<'a>(open: &'a Open, rows: &[&'a WordRow], size: Size) -> Element<'a, Message> {
    table::rows(open.scroll, size, rows.len(), |i| word_row(open, rows[i]))
}

/// One row: a cell per column and a 1 px line under it.
fn word_row<'a>(open: &'a Open, w: &'a WordRow) -> Element<'a, Message> {
    let word = table::line(&w.word).font(SANS_MEDIUM);
    let book = match book_title(&open.books, w) {
        Some(title) => table::line(title).style(theme::text_color(|c| c.muted)),
        None => table::line("—").style(theme::text_color(|c| c.faint)),
    };
    let device = table::line(&w.device_serial)
        .font(MONO)
        .size(12)
        .style(theme::text_color(|c| c.muted));
    let day = table::line(format::day(w.day())).style(theme::text_color(|c| c.muted));
    let cells = row![
        table::cell(word, width(Column::Word)),
        table::cell(book, width(Column::Book)),
        table::cell(device, width(Column::Device)),
        table::cell(day, width(Column::Day)),
    ]
    .height(Fill)
    .align_y(Center);
    column![
        container(cells).width(Fill).height(table::ROW),
        theme::hline(),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use epubsync_core::library::Stats;
    use epubsync_core::metadata::Metadata;

    fn book(id: i64, title: &str) -> Book {
        Book {
            id,
            revision: 1,
            metadata: Metadata {
                title: title.into(),
                ..Metadata::default()
            },
            stats: Stats::default(),
        }
    }

    fn word(word: &str, book_id: Option<i64>, book_title: Option<&str>) -> WordRow {
        WordRow {
            word: word.into(),
            device_serial: "N123".into(),
            book_id,
            volume_id: "file:///mnt/onboard/EpubSync/1.kepub.epub".into(),
            book_title: book_title.map(str::to_string),
            dict_suffix: None,
            looked_up_at: "2026-09-08T10:00:00Z".into(),
        }
    }

    #[test]
    fn the_library_title_wins_over_the_device_title() {
        let books = [book(1, "Emma")];
        let w = word("vex", Some(1), Some("emma (old)"));
        assert_eq!(book_title(&books, &w), Some("Emma"));
    }

    #[test]
    fn a_word_from_outside_the_library_keeps_the_device_title() {
        let books = [book(1, "Emma")];
        let w = word("vex", None, Some("Some Store Book"));
        assert_eq!(book_title(&books, &w), Some("Some Store Book"));
        let bare = word("vex", Some(9), None);
        assert_eq!(book_title(&books, &bare), None);
    }

    #[test]
    fn an_empty_filter_keeps_every_word_in_order() {
        let books = [book(1, "Emma")];
        let words = [word("vex", Some(1), None), word("hale", None, None)];
        let picked: Vec<&str> = select(&words, &books, "  ")
            .iter()
            .map(|w| w.word.as_str())
            .collect();
        assert_eq!(picked, ["vex", "hale"]);
    }

    #[test]
    fn the_filter_matches_the_word_or_the_title() {
        let books = [book(1, "Emma")];
        let words = [
            word("vex", Some(1), None),
            word("hale", None, Some("Persuasion")),
            word("emmanuel", None, None),
        ];
        let pick = |f: &str| -> Vec<&str> {
            select(&words, &books, f)
                .iter()
                .map(|w| w.word.as_str())
                .collect()
        };
        assert_eq!(pick("EMM"), ["vex", "emmanuel"]);
        assert_eq!(pick("persua"), ["hale"]);
        assert_eq!(pick("zzz"), Vec::<&str>::new());
    }
}
