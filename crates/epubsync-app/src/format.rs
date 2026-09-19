//! The small text formatters the panes share.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use epubsync_core::device::ReadStatus;
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

/// A time taken, in whole seconds: "8 s", "1 min 14 s", "2 h 5 min". The
/// largest unit leads, and one smaller unit follows unless it is zero.
pub fn elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    let (big, big_unit, small, small_unit) = match secs {
        s if s < 60 => return format!("{s} s"),
        s if s < 3600 => (s / 60, "min", s % 60, "s"),
        s => (s / 3600, "h", (s % 3600) / 60, "min"),
    };
    if small == 0 {
        format!("{big} {big_unit}")
    } else {
        format!("{big} {big_unit} {small} {small_unit}")
    }
}

/// The word for a progress status.
pub fn status(status: ReadStatus) -> &'static str {
    match status {
        ReadStatus::Unread => "unread",
        ReadStatus::Reading => "reading",
        ReadStatus::Finished => "finished",
    }
}

/// The series name and number as "Sherlock Holmes #4", or the name alone.
pub fn series_tag(series: &Series) -> String {
    match series.number {
        Some(n) => format!("{} #{}", series.name, format_series_number(n)),
        None => series.name.clone(),
    }
}

/// A count with a comma every three digits: 121970 becomes "121,970".
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A Flesch reading ease score as its rounded number and the word Flesch
/// gave that band: "61, standard".
pub fn reading_ease(score: f64) -> String {
    let band = match score {
        s if s >= 90.0 => "very easy",
        s if s >= 80.0 => "easy",
        s if s >= 70.0 => "fairly easy",
        s if s >= 60.0 => "standard",
        s if s >= 50.0 => "fairly difficult",
        s if s >= 30.0 => "difficult",
        _ => "very difficult",
    };
    format!("{score:.0}, {band}")
}

/// The authors' display names joined with " & ".
pub fn authors(authors: &[Author]) -> String {
    let names: Vec<&str> = authors.iter().map(|a| a.name.as_str()).collect();
    names.join(" & ")
}

/// A reading time in seconds as "3 h 20 min", "20 min", or "less than a
/// minute".
pub fn duration(seconds: i64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "less than a minute".to_string(),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The four-digit year of the system clock, in UTC, as "2026". A clock
/// before 1970 counts as 1970.
pub fn this_year() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    year_of_days((seconds / 86_400) as i64).to_string()
}

/// The civil year of a count of days since 1970-01-01, by the
/// days-to-civil arithmetic of Howard Hinnant's date algorithms. Only the
/// year is kept.
fn year_of_days(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // The algorithm's year starts in March, so January and February
    // belong to the next civil year.
    let month = (5 * day_of_year + 2) / 153;
    if month >= 10 { year + 1 } else { year }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_leads_with_the_largest_unit() {
        assert_eq!(elapsed(Duration::from_secs(0)), "0 s");
        assert_eq!(elapsed(Duration::from_secs(59)), "59 s");
        assert_eq!(elapsed(Duration::from_secs(60)), "1 min");
        assert_eq!(elapsed(Duration::from_secs(74)), "1 min 14 s");
        assert_eq!(elapsed(Duration::from_secs(3600)), "1 h");
        assert_eq!(elapsed(Duration::from_secs(7500)), "2 h 5 min");
    }

    #[test]
    fn duration_rounds_down_to_minutes() {
        assert_eq!(duration(0), "less than a minute");
        assert_eq!(duration(59), "less than a minute");
        assert_eq!(duration(60), "1 min");
        assert_eq!(duration(1200), "20 min");
        assert_eq!(duration(3600), "1 h 0 min");
        assert_eq!(duration(12_000), "3 h 20 min");
        assert_eq!(duration(90_000), "25 h 0 min");
    }

    #[test]
    fn year_of_days_crosses_the_new_year_and_the_leap_day() {
        assert_eq!(year_of_days(0), 1970);
        assert_eq!(year_of_days(364), 1970);
        assert_eq!(year_of_days(365), 1971);
        // 2024 was a leap year: day 19723 is 1 January, 19782 is 29 February.
        assert_eq!(year_of_days(19_722), 2023);
        assert_eq!(year_of_days(19_723), 2024);
        assert_eq!(year_of_days(19_782), 2024);
        assert_eq!(year_of_days(19_783), 2024);
        assert_eq!(year_of_days(20_453), 2025);
        assert_eq!(year_of_days(20_454), 2026);
        assert_eq!(this_year().len(), 4);
    }

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
        assert_eq!(status(ReadStatus::Unread), "unread");
        assert_eq!(status(ReadStatus::Reading), "reading");
        assert_eq!(status(ReadStatus::Finished), "finished");
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
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(121970), "121,970");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn reading_ease_rounds_and_names_the_band() {
        assert_eq!(reading_ease(60.95), "61, standard");
        assert_eq!(reading_ease(92.0), "92, very easy");
        assert_eq!(reading_ease(59.6), "60, fairly difficult");
        assert_eq!(reading_ease(12.3), "12, very difficult");
    }
}
