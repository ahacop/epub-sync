//! The small text formatters the panes share.

use epubsync_core::metadata::{Author, Series, format_series_number};

/// "2026-09-08" becomes "8 Sep". The input is the first ten characters of
/// a progress row's `last_read`. Text that is not a date comes back as is.
pub fn day(date: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = date.split('-').skip(1);
    let month = parts.next().and_then(|m| m.parse::<usize>().ok());
    let day = parts.next().and_then(|d| d.parse::<u32>().ok());
    match (month, day) {
        (Some(m), Some(d)) if (1..=12).contains(&m) => format!("{d} {}", MONTHS[m - 1]),
        _ => date.to_string(),
    }
}

/// The word for a progress status.
pub fn status(status: i64) -> &'static str {
    match status {
        0 => "unread",
        1 => "reading",
        2 => "finished",
        _ => "?",
    }
}

/// The series name and number as "Sherlock Holmes #4", or the name alone.
pub fn series_tag(series: &Series) -> String {
    match series.number {
        Some(n) => format!("{} #{}", series.name, format_series_number(n)),
        None => series.name.clone(),
    }
}

/// The authors' display names joined with " & ".
pub fn authors(authors: &[Author]) -> String {
    let names: Vec<&str> = authors.iter().map(|a| a.name.as_str()).collect();
    names.join(" & ")
}

/// The title in lower case without a leading "The ", "A ", or "An ", for
/// the title sort.
pub fn title_key(title: &str) -> String {
    let lower = title.to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_names_each_month() {
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        for (i, name) in months.iter().enumerate() {
            let date = format!("2026-{:02}-08", i + 1);
            assert_eq!(day(&date), format!("8 {name}"));
        }
    }

    #[test]
    fn day_drops_the_leading_zero() {
        assert_eq!(day("2026-09-08"), "8 Sep");
        assert_eq!(day("2026-12-25"), "25 Dec");
    }

    #[test]
    fn day_keeps_text_that_is_not_a_date() {
        assert_eq!(day("soon"), "soon");
        assert_eq!(day("2026-13-01"), "2026-13-01");
    }

    #[test]
    fn status_words() {
        assert_eq!(status(0), "unread");
        assert_eq!(status(1), "reading");
        assert_eq!(status(2), "finished");
        assert_eq!(status(7), "?");
    }

    #[test]
    fn series_tag_with_and_without_a_number() {
        let numbered = Series {
            name: "Sherlock Holmes".into(),
            number: Some(4.0),
        };
        assert_eq!(series_tag(&numbered), "Sherlock Holmes #4");
        let half = Series {
            name: "Barsetshire".into(),
            number: Some(2.5),
        };
        assert_eq!(series_tag(&half), "Barsetshire #2.5");
        let bare = Series {
            name: "Martian".into(),
            number: None,
        };
        assert_eq!(series_tag(&bare), "Martian");
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
