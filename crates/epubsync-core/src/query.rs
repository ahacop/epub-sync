//! Picks and orders the books a display shows. The `list` command and the
//! viewer's table both filter and sort with the rules here, so the same
//! flags and the same column clicks give the same rows.
//!
//! A book's progress is the row read last on any device, and a book with
//! no row counts as unread. The sort puts a book with no value for the
//! key after every book that has one, and ties keep id order.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::device::ReadStatus;
use crate::library::{Book, ProgressRow};

/// What a book is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Id,
    /// The title in lower case without a leading "The", "A", or "An".
    Title,
    /// The first author's sort name.
    Author,
    /// The series name, then the number in the series.
    Series,
    Words,
    Ease,
    /// The percent of the latest progress row.
    Progress,
    /// The day of the latest progress row.
    LastRead,
}

/// The sort keys and the direction. The second key orders the books the
/// first key ties, and so on. Books equal on every key keep id order.
/// `descending` reverses the whole order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub keys: Vec<SortKey>,
    pub descending: bool,
}

impl Sort {
    /// One key, ascending.
    pub fn by(key: SortKey) -> Sort {
        Sort {
            keys: vec![key],
            descending: false,
        }
    }
}

impl Default for Sort {
    /// The title, ascending.
    fn default() -> Self {
        Sort::by(SortKey::Title)
    }
}

/// The rules a book has to meet to be shown. Each text is matched without
/// regard to case, and a text that is empty or only spaces is not applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Text in the title, in an author's name, or in the series name.
    pub text: String,
    pub title: String,
    pub author: String,
    pub series: String,
    pub status: Option<ReadStatus>,
}

impl Filter {
    /// Whether the filter applies no rule.
    pub fn is_empty(&self) -> bool {
        [&self.text, &self.title, &self.author, &self.series]
            .iter()
            .all(|t| t.trim().is_empty())
            && self.status.is_none()
    }
}

/// The filter and the sort a display applies to the library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub filter: Filter,
    pub sort: Sort,
}

impl Query {
    /// The books that meet the filter, in sort order. `books` is in id
    /// order, which the sort keeps between equal keys.
    pub fn select<'a>(
        &self,
        books: &'a [Book],
        progress: &BTreeMap<i64, Vec<ProgressRow>>,
    ) -> Vec<&'a Book> {
        let needles = Needles::from(&self.filter);
        let keys = |b: &Book| -> Vec<Option<Key>> {
            self.sort
                .keys
                .iter()
                .map(|&k| key(b, k, progress))
                .collect()
        };
        let mut rows: Vec<(Vec<Option<Key>>, &Book)> = books
            .iter()
            .filter(|b| needles.matches(b, progress))
            .map(|b| (keys(b), b))
            .collect();
        // The sort is stable, and the books are in id order.
        rows.sort_by(|(a, _), (b, _)| {
            let order = a
                .iter()
                .zip(b)
                .map(|(a, b)| compare(a, b))
                .find(|o| o.is_ne())
                .unwrap_or(Ordering::Equal);
            if self.sort.descending {
                order.reverse()
            } else {
                order
            }
        });
        rows.into_iter().map(|(_, b)| b).collect()
    }
}

/// The progress row with the greatest `last_read` for a book. A row
/// without `last_read` counts as the oldest.
pub fn latest(progress: &BTreeMap<i64, Vec<ProgressRow>>, book_id: i64) -> Option<&ProgressRow> {
    progress
        .get(&book_id)?
        .iter()
        .max_by_key(|p| p.last_read.as_deref())
}

/// The status of the latest progress row. A book with no row is unread.
pub fn status(progress: &BTreeMap<i64, Vec<ProgressRow>>, book_id: i64) -> ReadStatus {
    latest(progress, book_id).map_or(ReadStatus::Unread, |p| p.status)
}

/// The title in lower case without a leading "The ", "A ", or "An ".
pub fn title_key(title: &str) -> String {
    let lower = title.to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

/// The filter texts trimmed and in lower case, with the empty ones
/// dropped, so a select lowers each text once and not once per book.
struct Needles {
    text: Option<String>,
    title: Option<String>,
    author: Option<String>,
    series: Option<String>,
    status: Option<ReadStatus>,
}

impl Needles {
    fn from(filter: &Filter) -> Needles {
        let needle = |t: &str| Some(t.trim().to_lowercase()).filter(|t| !t.is_empty());
        Needles {
            text: needle(&filter.text),
            title: needle(&filter.title),
            author: needle(&filter.author),
            series: needle(&filter.series),
            status: filter.status,
        }
    }

    fn matches(&self, book: &Book, progress: &BTreeMap<i64, Vec<ProgressRow>>) -> bool {
        let m = &book.metadata;
        let in_title = |n: &str| m.title.to_lowercase().contains(n);
        let in_author = |n: &str| m.authors.iter().any(|a| a.name.to_lowercase().contains(n));
        let in_series = |n: &str| {
            m.series
                .as_ref()
                .is_some_and(|s| s.name.to_lowercase().contains(n))
        };
        self.text
            .as_deref()
            .is_none_or(|n| in_title(n) || in_author(n) || in_series(n))
            && self.title.as_deref().is_none_or(in_title)
            && self.author.as_deref().is_none_or(in_author)
            && self.series.as_deref().is_none_or(in_series)
            && self.status.is_none_or(|s| status(progress, book.id) == s)
    }
}

/// The sort key of one book. `None` means the book has no value for the
/// key, and sorts after every book that has one.
#[derive(Debug, PartialEq, PartialOrd)]
enum Key {
    Id(i64),
    Title(String),
    Author(String),
    Series(String, Option<f64>),
    Words(u64),
    Ease(f64),
    Percent(i64),
    Day(String),
}

fn key(book: &Book, sort: SortKey, progress: &BTreeMap<i64, Vec<ProgressRow>>) -> Option<Key> {
    let m = &book.metadata;
    match sort {
        SortKey::Id => Some(Key::Id(book.id)),
        SortKey::Title => Some(Key::Title(title_key(&m.title))),
        SortKey::Author => m
            .authors
            .first()
            .map(|a| Key::Author(a.sort.to_lowercase())),
        SortKey::Series => m
            .series
            .as_ref()
            .map(|s| Key::Series(s.name.to_lowercase(), s.number)),
        SortKey::Words => book.stats.word_count.map(Key::Words),
        SortKey::Ease => book.stats.reading_ease.map(Key::Ease),
        SortKey::Progress => latest(progress, book.id).map(|p| Key::Percent(p.percent)),
        SortKey::LastRead => latest(progress, book.id)
            .and_then(ProgressRow::day)
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

#[cfg(test)]
mod tests {
    use crate::library::Stats;
    use crate::metadata::{Author, Metadata, Series};

    use super::*;

    fn author(name: &str, sort: &str) -> Author {
        Author {
            name: name.into(),
            sort: sort.into(),
        }
    }

    fn book(
        id: i64,
        title: &str,
        author: Option<Author>,
        series: Option<Series>,
        stats: Stats,
    ) -> Book {
        Book {
            id,
            revision: 1,
            metadata: Metadata {
                title: title.into(),
                authors: author.into_iter().collect(),
                series,
                ..Metadata::default()
            },
            stats,
        }
    }

    fn progress(percent: i64, status: ReadStatus, last_read: Option<&str>) -> ProgressRow {
        ProgressRow {
            device_serial: "N123".into(),
            percent,
            status,
            last_read: last_read.map(|d| format!("{d}T10:00:00Z")),
        }
    }

    /// Three books: The Warden (Trollope, Barsetshire 1, 62% reading,
    /// 72,000 words, ease 61), Villette (Brontë, no series, no progress,
    /// 196,000 words, ease 55), and A Princess of Mars (Burroughs,
    /// Martian 1, 100% finished, no stats).
    fn library() -> (Vec<Book>, BTreeMap<i64, Vec<ProgressRow>>) {
        let books = vec![
            book(
                1,
                "The Warden",
                Some(author("Anthony Trollope", "Trollope, Anthony")),
                Some(Series {
                    name: "Chronicles of Barsetshire".into(),
                    number: Some(1.0),
                }),
                Stats {
                    word_count: Some(72_000),
                    reading_ease: Some(61.0),
                },
            ),
            book(
                2,
                "Villette",
                Some(author("Charlotte Brontë", "Brontë, Charlotte")),
                None,
                Stats {
                    word_count: Some(196_000),
                    reading_ease: Some(55.0),
                },
            ),
            book(
                3,
                "A Princess of Mars",
                Some(author("Edgar Rice Burroughs", "Burroughs, Edgar Rice")),
                Some(Series {
                    name: "Martian".into(),
                    number: Some(1.0),
                }),
                Stats::default(),
            ),
        ];
        let progress = BTreeMap::from([
            (
                1,
                vec![progress(62, ReadStatus::Reading, Some("2026-09-08"))],
            ),
            (
                3,
                vec![progress(100, ReadStatus::Finished, Some("2026-05-12"))],
            ),
        ]);
        (books, progress)
    }

    fn ids(query: &Query) -> Vec<i64> {
        let (books, progress) = library();
        query
            .select(&books, &progress)
            .iter()
            .map(|b| b.id)
            .collect()
    }

    fn sorted(key: SortKey) -> Query {
        Query {
            sort: Sort::by(key),
            ..Query::default()
        }
    }

    fn filtered(filter: Filter) -> Query {
        Query {
            filter,
            sort: Sort::by(SortKey::Id),
        }
    }

    #[test]
    fn default_order_is_by_title_without_articles() {
        // princess of mars, villette, warden
        assert_eq!(ids(&Query::default()), vec![3, 2, 1]);
    }

    #[test]
    fn descending_flips_the_order() {
        let mut query = Query::default();
        query.sort.descending = true;
        assert_eq!(ids(&query), vec![1, 2, 3]);
    }

    #[test]
    fn id_order_keeps_the_library_order() {
        assert_eq!(ids(&sorted(SortKey::Id)), vec![1, 2, 3]);
    }

    #[test]
    fn author_order_uses_the_sort_name() {
        // Brontë, Burroughs, Trollope
        assert_eq!(ids(&sorted(SortKey::Author)), vec![2, 3, 1]);
    }

    #[test]
    fn series_order_puts_a_book_without_a_series_last() {
        // Chronicles of Barsetshire, Martian, then Villette
        assert_eq!(ids(&sorted(SortKey::Series)), vec![1, 3, 2]);
    }

    #[test]
    fn words_order_puts_a_book_without_stats_last() {
        // 72,000, 196,000, then A Princess of Mars
        assert_eq!(ids(&sorted(SortKey::Words)), vec![1, 2, 3]);
    }

    #[test]
    fn ease_order_puts_a_book_without_stats_last() {
        // 55, 61, then A Princess of Mars
        assert_eq!(ids(&sorted(SortKey::Ease)), vec![2, 1, 3]);
    }

    #[test]
    fn progress_order_puts_a_book_without_a_row_last() {
        // 62%, 100%, then Villette
        assert_eq!(ids(&sorted(SortKey::Progress)), vec![1, 3, 2]);
    }

    #[test]
    fn last_read_order_puts_a_book_without_a_row_last() {
        // May, September, then Villette
        assert_eq!(ids(&sorted(SortKey::LastRead)), vec![3, 1, 2]);
    }

    #[test]
    fn a_second_key_orders_the_books_the_first_key_ties() {
        let trollope = || Some(author("Anthony Trollope", "Trollope, Anthony"));
        let books = vec![
            book(1, "The Warden", trollope(), None, Stats::default()),
            book(2, "Barchester Towers", trollope(), None, Stats::default()),
            book(
                3,
                "Villette",
                Some(author("Charlotte Brontë", "Brontë, Charlotte")),
                None,
                Stats::default(),
            ),
        ];
        let progress = BTreeMap::new();
        let ids = |keys: &[SortKey], descending: bool| -> Vec<i64> {
            let query = Query {
                sort: Sort {
                    keys: keys.to_vec(),
                    descending,
                },
                ..Query::default()
            };
            query
                .select(&books, &progress)
                .iter()
                .map(|b| b.id)
                .collect()
        };
        // Brontë, then Trollope's two books in id order.
        assert_eq!(ids(&[SortKey::Author], false), vec![3, 1, 2]);
        // Brontë, then Trollope's two books by title.
        assert_eq!(
            ids(&[SortKey::Author, SortKey::Title], false),
            vec![3, 2, 1]
        );
        // The whole order reversed, not one key.
        assert_eq!(ids(&[SortKey::Author, SortKey::Title], true), vec![1, 2, 3]);
        // No key at all keeps id order.
        assert_eq!(ids(&[], false), vec![1, 2, 3]);
    }

    #[test]
    fn latest_row_is_the_one_read_last() {
        let rows = BTreeMap::from([(
            1,
            vec![
                progress(10, ReadStatus::Reading, None),
                progress(62, ReadStatus::Reading, Some("2026-09-08")),
                progress(30, ReadStatus::Finished, Some("2026-07-02")),
            ],
        )]);
        assert_eq!(latest(&rows, 1).map(|p| p.percent), Some(62));
        assert_eq!(latest(&rows, 2), None);
        assert_eq!(status(&rows, 1), ReadStatus::Reading);
        assert_eq!(status(&rows, 2), ReadStatus::Unread);
    }

    #[test]
    fn text_matches_a_title_an_author_or_a_series() {
        let text = |t: &str| {
            filtered(Filter {
                text: t.into(),
                ..Filter::default()
            })
        };
        assert_eq!(ids(&text("bront")), vec![2]);
        assert_eq!(ids(&text("  BURROUGHS ")), vec![3]);
        assert_eq!(ids(&text("barset")), vec![1]);
        assert_eq!(ids(&text("mars")), vec![3]);
        assert_eq!(ids(&text("   ")), vec![1, 2, 3]);
    }

    #[test]
    fn each_field_matches_only_its_own_text() {
        let title = filtered(Filter {
            title: "warden".into(),
            ..Filter::default()
        });
        assert_eq!(ids(&title), vec![1]);
        let author = filtered(Filter {
            author: "warden".into(),
            ..Filter::default()
        });
        assert!(ids(&author).is_empty());
        let series = filtered(Filter {
            series: "martian".into(),
            ..Filter::default()
        });
        assert_eq!(ids(&series), vec![3]);
    }

    #[test]
    fn every_rule_has_to_match() {
        let query = filtered(Filter {
            author: "le guin".into(),
            series: "martian".into(),
            ..Filter::default()
        });
        assert!(ids(&query).is_empty());
        let query = filtered(Filter {
            author: "burroughs".into(),
            series: "martian".into(),
            ..Filter::default()
        });
        assert_eq!(ids(&query), vec![3]);
    }

    #[test]
    fn status_uses_the_latest_row_and_counts_no_row_as_unread() {
        let with = |s| {
            filtered(Filter {
                status: Some(s),
                ..Filter::default()
            })
        };
        assert_eq!(ids(&with(ReadStatus::Reading)), vec![1]);
        assert_eq!(ids(&with(ReadStatus::Finished)), vec![3]);
        assert_eq!(ids(&with(ReadStatus::Unread)), vec![2]);
    }

    #[test]
    fn an_empty_filter_applies_no_rule() {
        assert!(Filter::default().is_empty());
        assert!(
            Filter {
                text: "  ".into(),
                ..Filter::default()
            }
            .is_empty()
        );
        assert!(
            !Filter {
                status: Some(ReadStatus::Unread),
                ..Filter::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn title_key_drops_a_leading_article() {
        assert_eq!(title_key("The Warden"), "warden");
        assert_eq!(title_key("A Princess of Mars"), "princess of mars");
        assert_eq!(title_key("An Ideal Husband"), "ideal husband");
        assert_eq!(title_key("Villette"), "villette");
        assert_eq!(title_key("Theodore"), "theodore");
    }
}
